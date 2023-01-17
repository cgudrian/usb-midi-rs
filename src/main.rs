#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::{info};
use embassy_executor::Spawner;
use embassy_stm32::{Config, interrupt};
use embassy_stm32::time::{mhz};
use embassy_stm32::usb_otg::Driver;
use embassy_time::{Duration, Timer};
use embassy_usb::Builder;
use embassy_usb::class::cdc_acm::{State};
use futures::future::join;

use {defmt_rtt as _, panic_probe as _};

use crate::usb_midi::UsbMidiClass;

mod usb_midi;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("USB MIDI!");

    let mut config = Config::default();
    config.rcc.sys_ck = Some(mhz(168));
    config.rcc.pll48 = true;

    let p = embassy_stm32::init(config);

    let irq = interrupt::take!(OTG_FS);
    let mut ep_out_buffer = [0u8; 256];
    let driver = Driver::new_fs(p.USB_OTG_FS, irq, p.PA12, p.PA11, &mut ep_out_buffer);

    let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("MIDIbox");
    config.product = Some("USB-MIDI example");
    config.serial_number = Some("87654321");

    config.device_class = 0x00; // use class code from interface
    config.device_sub_class = 0x00; // unused
    config.device_protocol = 0x00; // unused
    config.max_packet_size_0 = 0x40; // 64 bytes

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut device_descriptor = [0; 256];
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256]; // binary object store
    let mut control_buf = [0; 64];

    let mut state = State::new(); // must come before the builder
    let mut midi_state = usb_midi::State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut device_descriptor,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut control_buf, None,
    );

    // Create classes on the builder
    // let mut class = CdcAcmClass::new(&mut builder, &mut state, 64);

    let mut midi_class = UsbMidiClass::new(&mut builder, &mut midi_state);
    let mut usb = builder.build();
    let usb_fut = usb.run();

    let hello_fut = async {
        loop {
            info!("Hello World!");
            Timer::after(Duration::from_secs(1)).await;
        }
    };

    // Run everything concurrently.
    // If we had made everything `'static` above instead, we could do this using separate tasks instead.
    join(usb_fut, hello_fut).await;
}