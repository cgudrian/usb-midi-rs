use defmt::info;
use embassy_usb::Builder;
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

pub struct State {}

impl State {
    pub fn new() -> Self {
        State {}
    }
}

pub struct UsbMidiClass {}

impl UsbMidiClass {
    pub fn new<'d, D: Driver<'d>>(builder: &mut Builder<'d, D>, state: &mut State) -> Self {
        let mut func = builder.function(USB_CLASS_AUDIO, AUDIO_SUBCLASS_AUDIOCONTROL, AUDIO_PROTOCOL_UNDEFINED);
        let mut intf = func.interface();
        let mut alt = intf.alt_setting(USB_CLASS_AUDIO, AUDIO_SUBCLASS_AUDIOCONTROL, AUDIO_PROTOCOL_UNDEFINED);
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

        alt.endpoint_bulk_out(64, EndpointExtra::audio(0, 0));

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x01,
                0x01,
                0x01,
            ],
        );

        alt.endpoint_bulk_in(64, EndpointExtra::audio(0, 0));

        alt.descriptor(
            CS_INTERFACE,
            &[
                0x01,
                0x01,
                0x03,
            ],
        );

        UsbMidiClass {}
    }
}