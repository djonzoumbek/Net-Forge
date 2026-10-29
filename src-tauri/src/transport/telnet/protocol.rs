//! Telnet protocol (RFC 854) as a pure state machine: bytes in, bytes out, no I/O.
//!
//! Supported options: ECHO and SUPPRESS-GO-AHEAD (server side), NAWS (RFC 1073,
//! window size) and TERMINAL-TYPE (RFC 1091) on our side. Everything else is
//! politely refused. Option states follow a simplified RFC 1143 "Q method", which
//! prevents the endless negotiation loops naive implementations fall into.

use crate::transport::TermSize;

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;

const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;

const TERMINAL_TYPE: &[u8] = b"XTERM-256COLOR";
/// Longest sub-negotiation we keep; longer ones are garbage and are dropped.
const MAX_SUB: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionState {
    No,
    /// We asked for it and wait for the answer: the answer must not be answered.
    WantYes,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    /// Received IAC.
    Iac,
    /// Received IAC WILL/WONT/DO/DONT, waiting for the option byte.
    Negotiation(u8),
    /// Inside IAC SB ... (sub-negotiation).
    Sub,
    /// Received IAC inside a sub-negotiation.
    SubIac,
    /// Received CR in data: a following NUL must be dropped (RFC 854).
    Cr,
}

/// Result of decoding bytes received from the server.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decoded {
    /// Terminal data to display (Telnet commands removed).
    pub data: Vec<u8>,
    /// Negotiation answers to send back to the server.
    pub reply: Vec<u8>,
}

pub struct TelnetCodec {
    state: State,
    sub: Vec<u8>,
    /// Options *we* perform (client side): NAWS, TERMINAL-TYPE, SGA.
    local: [OptionState; 256],
    /// Options the *server* performs: ECHO, SGA.
    remote: [OptionState; 256],
    size: TermSize,
}

impl TelnetCodec {
    pub fn new(size: TermSize) -> Self {
        Self {
            state: State::Data,
            sub: Vec::new(),
            local: [OptionState::No; 256],
            remote: [OptionState::No; 256],
            size,
        }
    }

    /// What we offer right after connecting, like most clients do. Servers that
    /// do not care simply refuse.
    pub fn initial_negotiation(&mut self) -> Vec<u8> {
        self.local[OPT_NAWS as usize] = OptionState::WantYes;
        self.local[OPT_TTYPE as usize] = OptionState::WantYes;
        self.remote[OPT_SGA as usize] = OptionState::WantYes;
        vec![IAC, WILL, OPT_NAWS, IAC, WILL, OPT_TTYPE, IAC, DO, OPT_SGA]
    }

    pub fn naws_enabled(&self) -> bool {
        self.local[OPT_NAWS as usize] == OptionState::Yes
    }

    /// New window size; returns the NAWS message if the server accepted NAWS.
    pub fn resize(&mut self, size: TermSize) -> Option<Vec<u8>> {
        self.size = size;
        self.naws_enabled().then(|| self.naws_message())
    }

    /// Encodes user input: IAC bytes are doubled, a bare CR becomes CR NUL.
    pub fn encode_input(&self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() + 8);
        let mut iter = input.iter().peekable();
        while let Some(&byte) = iter.next() {
            match byte {
                IAC => out.extend_from_slice(&[IAC, IAC]),
                b'\r' => {
                    out.push(b'\r');
                    if iter.peek() != Some(&&b'\n') {
                        out.push(0);
                    }
                }
                other => out.push(other),
            }
        }
        out
    }

    /// Decodes bytes from the server. Commands may be split across calls: the
    /// state is kept between them.
    pub fn decode(&mut self, input: &[u8]) -> Decoded {
        let mut out = Decoded::default();
        for &byte in input {
            self.state = match (self.state, byte) {
                (State::Data | State::Cr, IAC) => State::Iac,
                (State::Cr, 0) => State::Data,
                (State::Data | State::Cr, b'\r') => {
                    out.data.push(b'\r');
                    State::Cr
                }
                (State::Data | State::Cr, other) => {
                    out.data.push(other);
                    State::Data
                }

                (State::Iac, IAC) => {
                    out.data.push(IAC);
                    State::Data
                }
                (State::Iac, WILL | WONT | DO | DONT) => State::Negotiation(byte),
                (State::Iac, SB) => {
                    self.sub.clear();
                    State::Sub
                }
                // NOP, GA, AYT... nothing to do for a terminal.
                (State::Iac, _) => State::Data,

                (State::Negotiation(command), option) => {
                    self.negotiate(command, option, &mut out.reply);
                    State::Data
                }

                (State::Sub, IAC) => State::SubIac,
                (State::Sub, other) => {
                    if self.sub.len() < MAX_SUB {
                        self.sub.push(other);
                    }
                    State::Sub
                }
                (State::SubIac, SE) => {
                    self.subnegotiation(&mut out.reply);
                    State::Data
                }
                (State::SubIac, IAC) => {
                    if self.sub.len() < MAX_SUB {
                        self.sub.push(IAC);
                    }
                    State::Sub
                }
                // Malformed: abandon the sub-negotiation.
                (State::SubIac, _) => State::Data,
            };
        }
        out
    }

    fn negotiate(&mut self, command: u8, option: u8, reply: &mut Vec<u8>) {
        let index = option as usize;
        match command {
            // The server offers to perform an option.
            WILL => match self.remote[index] {
                OptionState::No if matches!(option, OPT_ECHO | OPT_SGA) => {
                    self.remote[index] = OptionState::Yes;
                    reply.extend_from_slice(&[IAC, DO, option]);
                }
                OptionState::No => reply.extend_from_slice(&[IAC, DONT, option]),
                OptionState::WantYes => self.remote[index] = OptionState::Yes,
                OptionState::Yes => {}
            },
            WONT => {
                if self.remote[index] == OptionState::Yes {
                    reply.extend_from_slice(&[IAC, DONT, option]);
                }
                self.remote[index] = OptionState::No;
            }
            // The server asks us to perform an option.
            DO => match self.local[index] {
                OptionState::No if matches!(option, OPT_NAWS | OPT_TTYPE | OPT_SGA) => {
                    self.local[index] = OptionState::Yes;
                    reply.extend_from_slice(&[IAC, WILL, option]);
                    self.enabled_locally(option, reply);
                }
                OptionState::No => reply.extend_from_slice(&[IAC, WONT, option]),
                OptionState::WantYes => {
                    self.local[index] = OptionState::Yes;
                    self.enabled_locally(option, reply);
                }
                OptionState::Yes => {}
            },
            DONT => {
                if self.local[index] == OptionState::Yes {
                    reply.extend_from_slice(&[IAC, WONT, option]);
                }
                self.local[index] = OptionState::No;
            }
            _ => {}
        }
    }

    fn enabled_locally(&self, option: u8, reply: &mut Vec<u8>) {
        if option == OPT_NAWS {
            reply.extend_from_slice(&self.naws_message());
        }
    }

    fn subnegotiation(&mut self, reply: &mut Vec<u8>) {
        if self.sub.as_slice() == [OPT_TTYPE, TTYPE_SEND]
            && self.local[OPT_TTYPE as usize] == OptionState::Yes
        {
            reply.extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_IS]);
            reply.extend_from_slice(TERMINAL_TYPE);
            reply.extend_from_slice(&[IAC, SE]);
        }
    }

    /// IAC SB NAWS <width:u16> <height:u16> IAC SE, with 255 bytes doubled.
    fn naws_message(&self) -> Vec<u8> {
        let mut out = vec![IAC, SB, OPT_NAWS];
        for byte in self
            .size
            .cols
            .to_be_bytes()
            .into_iter()
            .chain(self.size.rows.to_be_bytes())
        {
            out.push(byte);
            if byte == IAC {
                out.push(IAC);
            }
        }
        out.extend_from_slice(&[IAC, SE]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: TermSize = TermSize { cols: 80, rows: 24 };

    fn naws(cols: u16, rows: u16) -> Vec<u8> {
        TelnetCodec::new(TermSize { cols, rows }).naws_message()
    }

    #[test]
    fn plain_data_passes_through() {
        let mut c = TelnetCodec::new(SIZE);
        let d = c.decode(b"Router>");
        assert_eq!(d.data, b"Router>");
        assert!(d.reply.is_empty());
    }

    #[test]
    fn accepts_server_echo_and_sga_refuses_others() {
        let mut c = TelnetCodec::new(SIZE);
        let d = c.decode(&[IAC, WILL, OPT_ECHO, IAC, WILL, OPT_SGA, IAC, WILL, 42]);
        assert_eq!(
            d.reply,
            [IAC, DO, OPT_ECHO, IAC, DO, OPT_SGA, IAC, DONT, 42]
        );
        // Repeated offers are not answered again: no negotiation loop.
        assert!(c.decode(&[IAC, WILL, OPT_ECHO]).reply.is_empty());
    }

    #[test]
    fn naws_sent_when_requested_and_on_resize() {
        let mut c = TelnetCodec::new(SIZE);
        assert_eq!(
            c.resize(TermSize {
                cols: 100,
                rows: 30
            }),
            None,
            "not negotiated yet"
        );
        let d = c.decode(&[IAC, DO, OPT_NAWS]);
        let mut expected = vec![IAC, WILL, OPT_NAWS];
        expected.extend(naws(100, 30));
        assert_eq!(d.reply, expected);
        assert_eq!(
            c.resize(TermSize {
                cols: 132,
                rows: 43
            }),
            Some(naws(132, 43))
        );
    }

    #[test]
    fn answer_to_our_own_offer_is_not_answered() {
        let mut c = TelnetCodec::new(SIZE);
        c.initial_negotiation();
        // Server agrees to what we offered: only the NAWS data is sent.
        let d = c.decode(&[IAC, DO, OPT_NAWS, IAC, WILL, OPT_SGA]);
        assert_eq!(d.reply, naws(80, 24));
        assert!(c.naws_enabled());
    }

    #[test]
    fn server_refusing_naws_disables_it() {
        let mut c = TelnetCodec::new(SIZE);
        c.initial_negotiation();
        assert!(c.decode(&[IAC, DONT, OPT_NAWS]).reply.is_empty());
        assert_eq!(c.resize(TermSize { cols: 90, rows: 30 }), None);
    }

    #[test]
    fn naws_doubles_255_bytes() {
        // 255 columns: the low byte equals IAC and must be escaped.
        assert_eq!(
            naws(255, 24),
            [IAC, SB, OPT_NAWS, 0, 255, 255, 0, 24, IAC, SE]
        );
    }

    #[test]
    fn terminal_type_is_sent_on_request() {
        let mut c = TelnetCodec::new(SIZE);
        c.decode(&[IAC, DO, OPT_TTYPE]);
        let d = c.decode(&[IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE]);
        let mut expected = vec![IAC, SB, OPT_TTYPE, TTYPE_IS];
        expected.extend_from_slice(b"XTERM-256COLOR");
        expected.extend_from_slice(&[IAC, SE]);
        assert_eq!(d.reply, expected);
    }

    #[test]
    fn commands_split_across_reads() {
        let mut c = TelnetCodec::new(SIZE);
        let a = c.decode(&[b'a', IAC]);
        let b = c.decode(&[WILL]);
        let d = c.decode(&[OPT_ECHO, b'b']);
        assert_eq!([a.data, b.data, d.data].concat(), b"ab");
        assert_eq!(d.reply, [IAC, DO, OPT_ECHO]);
    }

    #[test]
    fn escaped_iac_and_cr_nul_in_data() {
        let mut c = TelnetCodec::new(SIZE);
        let d = c.decode(&[b'x', IAC, IAC, b'\r', 0, b'y', b'\r', b'\n']);
        assert_eq!(d.data, [b'x', 255, b'\r', b'y', b'\r', b'\n']);
        // CR at the end of a read, NUL at the start of the next one.
        let mut c = TelnetCodec::new(SIZE);
        let a = c.decode(b"z\r");
        let b = c.decode(&[0, b'w']);
        assert_eq!([a.data, b.data].concat(), b"z\rw");
    }

    #[test]
    fn utf8_bytes_are_untouched() {
        let mut c = TelnetCodec::new(SIZE);
        assert_eq!(c.decode("é€✓".as_bytes()).data, "é€✓".as_bytes());
    }

    #[test]
    fn input_encoding() {
        let c = TelnetCodec::new(SIZE);
        assert_eq!(c.encode_input(b"show run\r"), b"show run\r\0");
        assert_eq!(c.encode_input(b"a\r\nb"), b"a\r\nb");
        assert_eq!(c.encode_input(&[0x03]), [0x03], "Ctrl+C unchanged");
        assert_eq!(c.encode_input(&[255]), [255, 255]);
    }

    #[test]
    fn oversized_subnegotiation_is_bounded() {
        let mut c = TelnetCodec::new(SIZE);
        let mut input = vec![IAC, SB, OPT_TTYPE];
        input.extend(std::iter::repeat_n(b'x', 10_000));
        input.extend_from_slice(&[IAC, SE, b'o', b'k']);
        let d = c.decode(&input);
        assert_eq!(d.data, b"ok");
        assert!(c.sub.len() <= MAX_SUB);
    }
}
