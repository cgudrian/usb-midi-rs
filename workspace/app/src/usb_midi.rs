#![no_std]

use core::mem::MaybeUninit;

use defmt::{write, Formatter};
use embassy_usb::control::ControlHandler;
use embassy_usb::descriptor::EndpointExtra;
use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use embassy_usb::Builder;
use heapless::Vec;

use {defmt_rtt as _, panic_probe as _};

const USB_CLASS_AUDIO: u8 = 0x01;
const AUDIO_SUBCLASS_AUDIOCONTROL: u8 = 0x01;
const AUDIO_SUBCLASS_MIDISTREAMING: u8 = 0x03;
const AUDIO_PROTOCOL_UNDEFINED: u8 = 0x00;

const CS_INTERFACE: u8 = 0x24;
const CS_ENDPOINT: u8 = 0x25;

const HEADER: u8 = 0x01;
const MS_HEADER: u8 = 0x01;
const MIDI_IN_JACK: u8 = 0x02;
const MIDI_OUT_JACK: u8 = 0x03;
const MS_GENERAL: u8 = 0x01;

const JACK_TYPE_EMBEDDED: u8 = 0x01;
const JACK_TYPE_EXTERNAL: u8 = 0x02;

pub const MAX_PACKET_SIZE: u16 = 64;
const MAX_MIDI_INTERFACE_COUNT: u8 = 8;

#[derive(defmt::Format, Copy, Clone, Eq, PartialEq)]
pub enum Event {
    Misc,
    Cable,
    SystemCommon2(u8, u8),
    SystemCommon3(u8, u8, u8),
    SysExStartCont(u8, u8, u8),
    SystemCommon1SysExEnd1(u8),
    SysExEnd2(u8, u8),
    SysExEnd3(u8, u8, u8),
    NoteOff(u8, Note, u8),
    NoteOn(u8, Note, u8),
    PolyKeyPress(u8, u8, u8),
    ControlChange(u8, u8, u8),
    ProgramChange(u8, u8),
    ChannelPressure(u8, u8),
    PitchBendChange(u8, u8, u8),
    SingleByte(u8),
}

impl Event {
    pub fn new(data: &[u8]) -> Event {
        assert_eq!(data.len(), 4);
        match data[0] & 0xf {
            0x0 => Event::Misc,
            0x1 => Event::Cable,
            0x2 => Event::SystemCommon2(data[1], data[2]),
            0x3 => Event::SystemCommon3(data[1], data[2], data[3]),
            0x4 => Event::SysExStartCont(data[1], data[2], data[3]),
            0x5 => Event::SystemCommon1SysExEnd1(data[1]),
            0x6 => Event::SysExEnd2(data[1], data[2]),
            0x7 => Event::SysExEnd3(data[1], data[2], data[3]),
            0x8 => Event::NoteOff(data[1], Note(data[2]), data[3]),
            0x9 => Event::NoteOn(data[1], Note(data[2]), data[3]),
            0xa => Event::PolyKeyPress(data[1], data[2], data[3]),
            0xb => Event::ControlChange(data[1], data[2], data[3]),
            0xc => Event::ProgramChange(data[1], data[2]),
            0xd => Event::ChannelPressure(data[1], data[2]),
            0xe => Event::PitchBendChange(data[1], data[2], data[3]),
            0xf => Event::SingleByte(data[1]),
            _ => panic!("now that's surprising"),
        }
    }
}

pub struct Control {
    string_offset: u8,
}

pub struct State {
    control: MaybeUninit<Control>,
}

impl State {
    pub fn new() -> Self {
        Self {
            control: MaybeUninit::uninit(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Note(u8);

const UPPER_NOTE_NAMES: [&str; 12] = [
    "C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-",
];
const LOWER_NOTE_NAMES: [&str; 12] = [
    "c-", "c#", "d-", "d#", "e-", "f-", "f#", "g-", "g#", "a-", "a#", "b-",
];

impl defmt::Format for Note {
    fn format(&self, fmt: Formatter) {
        let octave = (self.0 / 12) as isize - 2;
        let note = (self.0 % 12) as usize;
        let note = if octave < 0 {
            LOWER_NOTE_NAMES[note]
        } else {
            UPPER_NOTE_NAMES[note]
        };
        write!(fmt, "{}{}", note, octave.abs());
    }
}

// TODO Invent a static version of configuring the number of MIDI ports
impl ControlHandler for Control {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        let index: u8 = index.into();
        match index - self.string_offset {
            0 => Some("Port 1"),
            1 => Some("Port 2"),
            2 => Some("Port 3"),
            3 => Some("Port 4"),
            4 => Some("Port 5"),
            5 => Some("Port 6"),
            6 => Some("Port 7"),
            7 => Some("Port 8"),
            _ => None,
        }
    }
}

pub struct UsbMidiClass<'d, D: Driver<'d>, const N: usize> {
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
}

impl<'d, D: Driver<'d>, const N: usize> UsbMidiClass<'d, D, N> {
    pub fn new(builder: &mut Builder<'d, D>, state: &'d mut State) -> Self {
        assert!(N > 0, "interface count must be at least 1");
        assert!(
            N <= MAX_MIDI_INTERFACE_COUNT as usize,
            "interface count must not be greater than 8"
        );

        let mut func = builder.function(0, 0, 0);

        //
        // AudioControl Interface
        //
        let mut iface = func.interface();
        let mut alt = iface.alt_setting(
            USB_CLASS_AUDIO,
            AUDIO_SUBCLASS_AUDIOCONTROL,
            AUDIO_PROTOCOL_UNDEFINED,
        );
        alt.descriptor(
            CS_INTERFACE,
            &[
                HEADER, 0x00, // revision 1.0 (LSB)
                0x01, // revision 1.0 (MSB)
                0x09, // total size of class-specific descriptors (LSB)
                0x00, // total size of class-specific descriptors (MSB)
                0x01, // number of streaming interfaces
                0x01, // MS interface 1 belongs to this AC interface
            ],
        );

        //
        // MIDIStreaming Interface
        //
        let mut iface = func.interface();

        // reserve string indices for port names
        let mut port_names = [0u8; N];
        for idx in &mut port_names {
            *idx = iface.string().into();
        }

        let control = state.control.write(Control {
            string_offset: port_names[0],
        });
        iface.handler(control);

        let mut alt = iface.alt_setting(
            USB_CLASS_AUDIO,
            AUDIO_SUBCLASS_MIDISTREAMING,
            AUDIO_PROTOCOL_UNDEFINED,
        );

        // Class-specific MS Interface Descriptor
        // TODO: This is ugly as hell. I do not want to count bytes.
        let total_cs_descriptor_length =
            7 + (N as u16) * (6 + 6 + 9 + 9) + 9 + (4 + (N as u16)) + 9 + (4 + (N as u16));
        alt.descriptor(
            CS_INTERFACE,
            &[
                MS_HEADER,
                0x00,                                    // revision (LSB)
                0x01,                                    // revision (MSB)
                total_cs_descriptor_length as u8, // total size of class-specific descriptors (LSB)
                (total_cs_descriptor_length >> 8) as u8, // total size of class-specific descriptors (LSB)
            ],
        );

        let mut output_descriptor: Vec<u8, 10> = Vec::from_slice(&[MS_GENERAL, N as u8]).unwrap();

        let mut input_descriptor: Vec<u8, 10> = Vec::from_slice(&[MS_GENERAL, N as u8]).unwrap();

        for i in 0..N {
            let offset = i * 4;
            let jack_id_in_embedded = (offset + 0x01) as u8;
            let jack_id_in_external = (offset + 0x02) as u8;
            let jack_id_out_embedded = (offset + 0x03) as u8;
            let jack_id_out_external = (offset + 0x04) as u8;

            // MIDI IN Jack Descriptor (Embedded)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    MIDI_IN_JACK,
                    JACK_TYPE_EMBEDDED,
                    jack_id_in_embedded,
                    port_names[i], // iJack
                ],
            );
            output_descriptor.push(jack_id_in_embedded).unwrap();

            // MIDI Adapter MIDI IN Jack Descriptor (External)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    MIDI_IN_JACK,
                    JACK_TYPE_EXTERNAL,
                    jack_id_in_external,
                    0x00, // iJack
                ],
            );

            // MIDI Adapter MIDI OUT Jack Descriptor (Embedded)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    MIDI_OUT_JACK,
                    JACK_TYPE_EMBEDDED,
                    jack_id_out_embedded,
                    0x01,                // number of input pins of this jack
                    jack_id_in_external, // id of the entity to which this pin is connected
                    0x01, // output pin number of the entity to which this input pin is connected
                    port_names[i], // iJack
                ],
            );
            input_descriptor.push(jack_id_out_embedded).unwrap();

            // MIDI Adapter MIDI OUT Jack Descriptor (External)
            alt.descriptor(
                CS_INTERFACE,
                &[
                    MIDI_OUT_JACK, // l
                    JACK_TYPE_EXTERNAL,
                    jack_id_out_external,
                    0x01,                // number of input pins of this jack
                    jack_id_in_embedded, // id of the entity to which this pin is connected
                    0x01, // output pin number of the entity to which this input pin is connected
                    0x00, // iJack
                ],
            );
        }

        // Standard Bulk OUT Endpoint Descriptor
        let read_ep = alt.endpoint_bulk_out(MAX_PACKET_SIZE, EndpointExtra::audio(0, 0));
        alt.descriptor(CS_ENDPOINT, output_descriptor.as_slice());

        let write_ep = alt.endpoint_bulk_in(MAX_PACKET_SIZE, EndpointExtra::audio(0, 0));
        alt.descriptor(CS_ENDPOINT, input_descriptor.as_slice());

        UsbMidiClass { read_ep, write_ep }
    }

    pub async fn read_packets(&mut self, data: &mut [u8]) -> Result<usize, EndpointError> {
        self.read_ep.read(data).await
    }

    pub async fn write_packet(&mut self, data: &[u8]) -> Result<(), EndpointError> {
        self.write_ep.write(data).await
    }

    pub async fn wait_connection(&mut self) {
        self.read_ep.wait_enabled().await
    }
}

impl<'d, D: Driver<'d>> UsbMidiClass<'d, D, 2> {
    pub fn split_cables(&self) -> (u8, u8) {
        (1, 2)
    }
}
