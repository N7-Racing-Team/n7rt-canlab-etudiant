//! Framing properties on generated inputs; byte-exact conformance stays in the restricted test harness.
use n7rt_link::*;

fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    (0..len).map(|_| { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state as u8 }).collect()
}

#[test]
fn cobs_round_trips_generated_bytes_and_never_emits_a_delimiter() {
    for len in 0..1024 {
        let data = bytes(len as u64 + 1, len);
        let encoded = cobs_encode(&data);
        assert!(!encoded.contains(&DELIMITER));
        assert!(encoded.len() <= data.len() + data.len() / 254 + 1);
        assert_eq!(cobs_decode(&encoded).unwrap(), data);
    }
}

#[test]
fn receiver_chunk_boundaries_do_not_change_generated_messages() {
    let messages: Vec<_> = (0..128u32).map(|i| Message::PcHeartbeat { host_uptime_ms: i.wrapping_mul(7919) }).collect();
    let mut encoder = Encoder::new();
    let wire: Vec<u8> = messages.iter().flat_map(|message| encoder.encode(message).unwrap().1).collect();
    for size in 1..64 {
        let mut receiver = Receiver::new(Direction::PcToBoard);
        let mut received = Vec::new();
        for chunk in wire.chunks(size) {
            receiver.feed(chunk, |event| match event {
                Event::Packet(packet) => received.push(packet.message),
                other => panic!("unexpected framing event: {other:?}"),
            });
        }
        assert_eq!(received, messages);
        assert_eq!(receiver.pending(), 0);
    }
}

#[test]
fn corrupted_payload_is_rejected_and_following_packet_resynchronizes() {
    for seq in 0..128u16 {
        let message = Message::PcHeartbeat { host_uptime_ms: u32::from(seq).wrapping_mul(7919) };
        let payload = message.encode_payload_with_extension(&[]).unwrap();
        let packet = build_packet(message.type_code(), seq, &payload);
        let mut damaged = packet.clone();
        damaged[HEADER_SIZE] ^= 1;
        let wire: Vec<u8> = encode_wire(&damaged).into_iter().chain(encode_wire(&packet)).collect();
        let events = Receiver::new(Direction::PcToBoard).feed_collect(&wire);
        assert!(matches!(events.first(), Some(Event::FrameError(FramingError::Crc))));
        assert!(matches!(events.last(), Some(Event::Packet(packet)) if packet.message == message));
        assert_eq!(events.len(), 2);
    }
}
