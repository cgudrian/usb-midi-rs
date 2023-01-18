#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::{Config, interrupt, Peripheral, Peripherals, usb_otg};
use embassy_stm32::peripherals::USB_OTG_FS;
use embassy_stm32::time::mhz;
use embassy_stm32::usb_otg::{DmPin, DpPin, Driver, Instance};
use embassy_time::{Duration, Timer};
use embassy_usb::{Builder, UsbDevice};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use futures::future::join;

use {defmt_rtt as _, panic_probe as _};

use crate::usb_midi::UsbMidiClass;

mod usb_midi;

struct UsbBuffers {
    device_descriptor: [u8; 256],
    config_descriptor: [u8; 256],
    bos_descriptor: [u8; 64],
    control_buf: [u8; 64],
    ep_out_buffer: [u8; 256],
}

impl UsbBuffers {
    fn new() -> UsbBuffers {
        let mut device_descriptor = [0; 256];
        let mut config_descriptor = [0; 256];
        let mut bos_descriptor = [0; 64];
        let mut control_buf = [0; 64];
        let mut ep_out_buffer= [0; 256];

        UsbBuffers {
            device_descriptor,
            config_descriptor,
            bos_descriptor,
            control_buf,
            ep_out_buffer,
        }
    }
}

fn build_usb_devices<'d, P1, P2>(usb: USB_OTG_FS,
                                 p1: P1,
                                 p2: P2,
                                 buffers: &'d mut UsbBuffers,
) -> (UsbMidiClass<'d, Driver<'d, USB_OTG_FS>>, UsbDevice<'d, Driver<'d, USB_OTG_FS>>)
    where P1: Peripheral + 'd,
          P1::P: DpPin<USB_OTG_FS>,
          P2: Peripheral + 'd,
          P2::P: DmPin<USB_OTG_FS>
{
    let irq = interrupt::take!(OTG_FS);
    let driver = Driver::new_fs(usb, irq, p1, p2, &mut buffers.ep_out_buffer);

    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("MIDIbox");
    config.product = Some("USB-MIDI example");
    config.serial_number = Some("87654321");

    config.device_class = 0x00; // use class code from interface
    config.device_sub_class = 0x00; // unused
    config.device_protocol = 0x00; // unused
    config.max_packet_size_0 = 0x40; // 64 bytes

    config.self_powered = false;
    config.max_power = 100;

    let mut builder = Builder::new(
        driver,
        config,
        &mut buffers.device_descriptor,
        &mut buffers.config_descriptor,
        &mut buffers.bos_descriptor,
        &mut buffers.control_buf,
        None,
    );

    // Create classes on the builder
    // let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);
    let midi_class = UsbMidiClass::new::<2>(&mut builder);

    let usb = builder.build();

    (midi_class, usb)
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("USB MIDI!");

    let mut config = Config::default();
    config.rcc.sys_ck = Some(mhz(180));
    config.rcc.pll48 = true;

    let p = embassy_stm32::init(config);

    let mut buffers = UsbBuffers::new();

    let (mut midi_class, mut usb) = build_usb_devices(
        p.USB_OTG_FS, p.PA12, p.PA11,
        &mut buffers);

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
