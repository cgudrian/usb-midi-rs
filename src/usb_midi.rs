use core::mem::MaybeUninit;

use defmt::{debug, info};
use embassy_usb::{Builder, control};
use embassy_usb::control::{ControlHandler, InResponse, OutResponse, Request};
use embassy_usb::descriptor::EndpointExtra;
use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use heapless::Vec;

const USB_CLASS_AUDIO: u8 = 0x01;
const AUDIO_SUBCLASS_AUDIOCONTROL: u8 = 0x01;
const AUDIO_SUBCLASS_AUDIOSTREAMING: u8 = 0x02;
const AUDIO_SUBCLASS_MIDISTREAMING: u8 = 0x03;
const AUDIO_PROTOCOL_UNDEFINED: u8 = 0x00;

const CS_INTERFACE: u8 = 0x24;
const CS_ENDPOINT: u8 = 0x25;

const AUDIO_TYPE_HEADER: u8 = 0x01;
const AUDIO_TYPE_MS: u8 = 0x01;

const ENDPOINT_OUT: u8 = 0x01;
const ENDPOINT_IN: u8 = 0x81;
const ENDPOINT_BULK: u8 = 0x02;

const MAX_PACKET_SIZE: u16 = 64;

const MAX_MIDI_INTERFACE_COUNT: usize = 8;

pub struct State<'a> {
    control1: MaybeUninit<Control<'a>>,
    control2: MaybeUninit<Control<'a>>,
    shared: ControlShared,
}

impl<'a> State<'a> {
    pub fn new() -> Self {
        Self {
            control1: MaybeUninit::uninit(),
            control2: MaybeUninit::uninit(),
            shared: Default::default(),
        }
    }
}

pub struct UsbMidiClass<'d, D: Driver<'d>> {
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
    control: &'d ControlShared,
}

struct Control<'a> {
    shared: &'a ControlShared,
}

struct ControlShared {}

impl Default for ControlShared {
    fn default() -> Self {
        ControlShared {}
    }
}

impl<'a> Control<'a> {
    fn shared(&mut self) -> &'a ControlShared {
        self.shared
    }
}

impl<'d> ControlHandler for Control<'d> {
    fn reset(&mut self) {
        debug!("reset");
    }

    fn control_out(&mut self, req: Request, _data: &[u8]) -> OutResponse {
        debug!("control_out: {}", req);
        OutResponse::Accepted
    }

    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> InResponse<'a> {
        debug!("control_in: {}", req);
        InResponse::Accepted(buf)
    }
}

impl<'d, D: Driver<'d>> UsbMidiClass<'d, D> {
    pub fn new<const INTF_COUNT: usize>(builder: &mut Builder<'d, D>, state: &'d mut State<'d>) -> Self {
        assert!(INTF_COUNT > 0, "interface count must be at least 1");
        assert!(INTF_COUNT <= MAX_MIDI_INTERFACE_COUNT, "interface count must not be greater than 8");

        let control = state.control1.write(Control { shared: &state.shared });
        let control_shared = &state.shared;

        let mut func = builder.function(USB_CLASS_AUDIO, AUDIO_SUBCLASS_AUDIOCONTROL, AUDIO_PROTOCOL_UNDEFINED);
        let mut iface = func.interface();
        iface.handler(control);

        let mut alt = iface.alt_setting(USB_CLASS_AUDIO, AUDIO_SUBCLASS_AUDIOCONTROL, AUDIO_PROTOCOL_UNDEFINED);
        alt.descriptor(
            CS_INTERFACE,
            &[
                AUDIO_TYPE_HEADER,
                0x00,
                0x01,
                0x09, // total size of class-specific descriptors (LSB)
                0x00, // (MSB)
                0x01, // number of streaming interfaces
                0x01, // MS interface 1 belongs to this AC interface
            ],
        );

        let control = state.control2.write(Control { shared: &state.shared });
        let mut iface = func.interface();
        iface.handler(control);

        let mut alt = iface.alt_setting(USB_CLASS_AUDIO, AUDIO_SUBCLASS_MIDISTREAMING, AUDIO_PROTOCOL_UNDEFINED);
        let descriptor_size = 7 + INTF_COUNT * (6 + 6 + 9 + 9) + 9 + (4 + INTF_COUNT) + 9 + (4 + INTF_COUNT);

        let mut output_descriptor: Vec<u8, 10> = Vec::from_slice(&[
            0x01,
            INTF_COUNT as u8,
        ]).unwrap();

        let mut input_descriptor: Vec<u8, 10> = Vec::from_slice(&[
            0x1,
            INTF_COUNT as u8,
        ]).unwrap();

        // Class-specific MS Interface Descriptor
        alt.descriptor(
            CS_INTERFACE,
            &[
                AUDIO_TYPE_MS,
                0x00, // revision (LSB)
                0x01, // revision (MSB)
                descriptor_size as u8,
                (descriptor_size >> 8) as u8,
            ],
        );

        for i in 0..INTF_COUNT {
            let midi_in_embedded = (0x01 + i * 4) as u8;
            let midi_in_external = (0x02 + i * 4) as u8;
            let midi_out_embedded = (0x03 + i * 4) as u8;
            let midi_out_external = (0x04 + i * 4) as u8;

            // MIDI IN Jack Descriptor (Embedded)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    0x02,
                    0x01,
                    midi_in_embedded,
                    0x00,
                ],
            );
            output_descriptor.push(midi_in_embedded).unwrap();

            // MIDI Adapter MIDI IN Jack Descriptor (External)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    0x02,
                    0x02,
                    midi_in_external,
                    0x00,
                ],
            );

            // MIDI Adapter MIDI OUT Jack Descriptor (Embedded)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    0x03,
                    0x01,
                    midi_out_embedded,
                    0x01,
                    midi_in_external,
                    0x01,
                    0x00,
                ],
            );
            input_descriptor.push(midi_out_embedded).unwrap();

            // MIDI Adapter MIDI OUT Jack Descriptor (External)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    0x03,
                    0x02,
                    midi_out_external,
                    0x01,
                    midi_in_embedded,
                    0x01,
                    0x00,
                ],
            );
        }

        // Standard Bulk OUT Endpoint Descriptor
        let read_ep = alt.endpoint_bulk_out(MAX_PACKET_SIZE, EndpointExtra::audio(0, 0));

        alt.descriptor(
            CS_ENDPOINT,
            output_descriptor.as_slice(),
        );

        let write_ep = alt.endpoint_bulk_in(MAX_PACKET_SIZE, EndpointExtra::audio(0, 0));

        alt.descriptor(
            CS_ENDPOINT,
            input_descriptor.as_slice(),
        );

        UsbMidiClass {
            read_ep,
            write_ep,
            control: control_shared,
        }
    }

    pub async fn read_packet(&mut self, data: &mut [u8]) -> Result<usize, EndpointError> {
        self.read_ep.read(data).await
    }

    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        self.write_ep.write(data).await
    }

    pub async fn wait_connection(&mut self) {
        self.read_ep.wait_enabled().await
    }
}