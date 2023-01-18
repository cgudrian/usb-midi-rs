use defmt::debug;

use embassy_usb::{Builder};
use embassy_usb::control::ControlHandler;

use embassy_usb::descriptor::EndpointExtra;
use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use heapless::Vec;

const USB_CLASS_AUDIO: u8 = 0x01;
const AUDIO_SUBCLASS_AUDIOCONTROL: u8 = 0x01;
const AUDIO_SUBCLASS_AUDIOSTREAMING: u8 = 0x02;
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

pub struct Handler {}

impl Handler {
    pub fn new() -> Self {
        Self {}
    }
}

impl ControlHandler for Handler {
    fn get_string(&mut self, index: StringIndex, lang_id: u16) -> Option<&str> {
        debug!("get_string index={} lang_id={}", index, lang_id);
        match index.into() {
            1 => Some("Eins"),
            2 => Some("Zwei"),
            3 => Some("Drei"),
            4 => Some("Vier"),
            5 => Some("Fünf"),
            6 => Some("Sechs"),
            7 => Some("Sieben"),
            8 => Some("Acht"),
            _ => None,
        }
    }
}

pub struct UsbMidiClass<'d, D: Driver<'d>, const N: usize> {
    read_ep: D::EndpointOut,
    write_ep: D::EndpointIn,
}

impl<'d, D: Driver<'d>, const N: usize> UsbMidiClass<'d, D, N> {
    pub fn new(builder: &mut Builder<'d, D>, handler: &'d mut Handler) -> Self {
        assert!(N > 0, "interface count must be at least 1");
        assert!(N <= MAX_MIDI_INTERFACE_COUNT as usize, "interface count must not be greater than 8");

        let mut func = builder.function(0, 0, 0);

        //
        // AudioControl Interface
        //
        let mut iface = func.interface();
        let mut alt = iface.alt_setting(USB_CLASS_AUDIO, AUDIO_SUBCLASS_AUDIOCONTROL, AUDIO_PROTOCOL_UNDEFINED, Some(7.into()));
        alt.descriptor(
            CS_INTERFACE,
            &[
                HEADER,
                0x00, // revision 1.0 (LSB)
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
        iface.handler(handler);

        // reserve string indices for jack names
        let mut port_names = [0u8; N];
        for idx in &mut port_names {
            *idx = iface.string().into();
        }

        let mut alt = iface.alt_setting(USB_CLASS_AUDIO, AUDIO_SUBCLASS_MIDISTREAMING, AUDIO_PROTOCOL_UNDEFINED, Some(6.into()));

        // Class-specific MS Interface Descriptor
        let total_cs_descriptor_length = 7 + (N as u16) * (6 + 6 + 9 + 9) + 9 + (4 + (N as u16)) + 9 + (4 + (N as u16));
        alt.descriptor(
            CS_INTERFACE,
            &[
                MS_HEADER,
                0x00, // revision (LSB)
                0x01, // revision (MSB)
                total_cs_descriptor_length as u8, // total size of class-specific descriptors (LSB)
                (total_cs_descriptor_length >> 8) as u8, // total size of class-specific descriptors (LSB)
            ],
        );

        let mut output_descriptor: Vec<u8, 10> = Vec::from_slice(&[
            MS_GENERAL,
            N as u8,
        ]).unwrap();

        let mut input_descriptor: Vec<u8, 10> = Vec::from_slice(&[
            MS_GENERAL,
            N as u8,
        ]).unwrap();

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
                    0x01, // number of input pins of this jack
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
                    JACK_TYPE_EXTERNAL,
                    jack_id_out_external,
                    0x01, // number of input pins of this jack
                    jack_id_in_embedded, // id of the entity to which this pin is connected
                    0x01, // output pin number of the entity to which this input pin is connected
                    0x00, // iJack
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

impl<'d, D: Driver<'d>> UsbMidiClass<'d, D, 2> {
    pub fn split_cables(&self) -> (u8, u8) {
        (1, 2)
    }
}
