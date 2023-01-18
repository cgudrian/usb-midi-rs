#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{Config, interrupt, Peripheral};
use embassy_stm32::peripherals::USB_OTG_FS;
use embassy_stm32::time::mhz;
use embassy_stm32::usb_otg::{DmPin, DpPin, Driver, Instance};
use embassy_usb::{Builder, UsbDevice};
use futures::future::join;

use {defmt_rtt as _, panic_probe as _};

use crate::usb_midi::{Handler, MAX_PACKET_SIZE, UsbMidiClass};

mod usb_midi;

struct UsbDeviceBuilder {
    device_descriptor: [u8; 256],
    config_descriptor: [u8; 256],
    bos_descriptor: [u8; 64],
    control_buf: [u8; 64],
    ep_out_buffer: [u8; 256],
    handler: Handler,
}

impl UsbDeviceBuilder {
    fn new() -> UsbDeviceBuilder {
        let device_descriptor = [0; 256];
        let config_descriptor = [0; 256];
        let bos_descriptor = [0; 64];
        let control_buf = [0; 64];
        let ep_out_buffer = [0; 256];

        UsbDeviceBuilder {
            device_descriptor,
            config_descriptor,
            bos_descriptor,
            control_buf,
            ep_out_buffer,
            handler: Handler::new(),
        }
    }

    fn build<'a, UsbInstance, UsbPeripheral, Dp, Dm>(
        &'a mut self,
        usb: UsbPeripheral,
        irq: UsbInstance::Interrupt,
        dp: Dp,
        dm: Dm,
    ) -> (
        UsbMidiClass<Driver<UsbInstance>, 2>,
        UsbDevice<Driver<UsbInstance>>
    )
        where
            UsbInstance: Instance,
            UsbPeripheral: Peripheral<P=UsbInstance> + 'a,
            Dp: Peripheral + 'a,
            Dp::P: DpPin<UsbInstance>,
            Dm: Peripheral + 'a,
            Dm::P: DmPin<UsbInstance>,
    {
        let driver = Driver::new_fs(usb, irq, dp, dm, &mut self.ep_out_buffer);

        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("MIDIbox");
        config.product = Some("USB-MIDI example");
        config.serial_number = Some("87654321");

        let mut builder = Builder::new(
            driver,
            config,
            &mut self.device_descriptor,
            &mut self.config_descriptor,
            &mut self.bos_descriptor,
            &mut self.control_buf,
            None,
        );

        let midi_class = UsbMidiClass::new(&mut builder, &mut self.handler);
        let usb_device = builder.build();

        (midi_class, usb_device)
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("USB MIDI!");

    let mut config = Config::default();
    config.rcc.sys_ck = Some(mhz(180));
    config.rcc.pll48 = true;

    let p = embassy_stm32::init(config);

    let irq = interrupt::take!(OTG_FS);

    let mut usb_device_builder = UsbDeviceBuilder::new();

    let (mut midi_class, mut usb) = usb_device_builder.build(
        p.USB_OTG_FS,
        irq,
        p.PA12,
        p.PA11,
    );

    let usb_fut = usb.run();

    let midi_fut = async {
        loop {
            let mut buf = [0; 64];
            midi_class.wait_connection().await;
            info!("### Connected ###");
            loop {
                let cnt = midi_class.read_packet(&mut buf).await.unwrap();
                for c in buf[0..cnt].chunks_exact(4) {
                    info!("### got data: cable:{} cin:{} midi:{}", c[0] >> 4, c[0] & 0xf, c[1..=3]);
                }
                let _ = midi_class.write_packet(&[1 << 4 | 9, 147, 53, 124]).await;
            }
        }
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join(usb_fut, midi_fut).await;
}
