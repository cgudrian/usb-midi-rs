use core::mem::MaybeUninit;
use defmt::info;
use embassy_usb::{Builder, control};
use embassy_usb::control::{ControlHandler, InResponse, OutResponse, Request};
use embassy_usb::descriptor::EndpointExtra;
use embassy_usb::driver::Driver;

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

const NUM_MIDI_PORTS: u16 = 1;

pub struct State<'a> {
    control: MaybeUninit<Control<'a>>,
    shared: ControlShared,
}

impl<'a> State<'a> {
    pub fn new() -> Self {
        Self {
            control: MaybeUninit::uninit(),
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
    fn reset(&mut self) {}

    fn control_out(&mut self, req: Request, _data: &[u8]) -> OutResponse {
        OutResponse::Accepted
    }

    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> InResponse<'a> {
        InResponse::Accepted(buf)
    }
}

impl<'d, D: Driver<'d>> UsbMidiClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>, state: &'d mut State<'d>) -> Self {
        let control = state.control.write(Control { shared: &state.shared });
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

        let mut func = builder.function(USB_CLASS_AUDIO, AUDIO_SUBCLASS_MIDISTREAMING, AUDIO_PROTOCOL_UNDEFINED);
        let mut intf = func.interface();
        let mut alt = intf.alt_setting(USB_CLASS_AUDIO, AUDIO_SUBCLASS_MIDISTREAMING, AUDIO_PROTOCOL_UNDEFINED);

        let descriptor_size = 7 + NUM_MIDI_PORTS * (6 + 6 + 9 + 9) + 9 + (4 + NUM_MIDI_PORTS) + 9 + (4 + NUM_MIDI_PORTS);

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

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x02,
                0x01,
                0x01,
                0x00,
            ],
        );

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x02,
                0x02,
                0x02,
                0x00,
            ],
        );

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x03,
                0x01,
                0x03,
                0x01,
                0x02,
                0x01,
                0x00,
            ],
        );

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x03,
                0x02,
                0x04,
                0x01,
                0x01,
                0x01,
                0x00,
            ],
        );

        let read_ep = alt.endpoint_bulk_out(64, EndpointExtra::audio(0, 0));

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x01,
                0x01,
                0x01,
            ],
        );

        let write_ep = alt.endpoint_bulk_in(64, EndpointExtra::audio(0, 0));

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x01,
                0x01,
                0x03,
            ],
        );

        UsbMidiClass {
            read_ep,
            write_ep,
            control: control_shared,
        }
    }
}