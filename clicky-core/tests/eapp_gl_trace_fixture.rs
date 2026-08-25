use clicky_core::sys::eapp::{GlMemoryRegion, GlTraceFixture};

#[test]
fn tetris_gl_trace_fixture_has_expected_shape() {
    let fixture: GlTraceFixture =
        serde_json::from_str(include_str!("fixtures/eapp/tetris_gl_trace.json"))
            .expect("valid trace fixture json");

    assert_eq!(fixture.title, "66666");
    assert_eq!(fixture.file_vma_base, 0x1800_0000);
    assert_eq!(fixture.work_ram_base, 0x1000_0000);
    assert_eq!(fixture.stack_snapshot_len, 0x80);
    assert_eq!(fixture.pointer_snapshot_len, 0x80);
    assert_eq!(fixture.capture_start_frame, 0);
    assert_eq!(fixture.capture_end_frame, 50);
    assert_eq!(fixture.frames.len(), 5);

    let frame0 = &fixture.frames[0];
    assert_eq!(frame0.first_frame, 0);
    assert_eq!(frame0.repeat_count, 1);
    assert!(frame0.records.iter().any(|r| r.ordinal == 157));

    let frame2 = &fixture.frames[2];
    assert_eq!(frame2.first_frame, 2);
    assert_eq!(frame2.repeat_count, 1);
    assert!(frame2.records.iter().any(|r| r.ordinal == 45));
    assert!(frame2.records.iter().any(|r| r.ordinal == 4));
    assert!(frame2.records.iter().any(|r| r.ordinal == 99));
    assert!(frame2.records.iter().any(|r| r.ordinal == 158));

    let frame4 = &fixture.frames[4];
    assert_eq!(frame4.first_frame, 4);
    assert_eq!(frame4.last_frame, 50);
    assert_eq!(frame4.repeat_count, 47);
    assert!(frame4.records.iter().any(|r| r.ordinal == 37));
    assert!(frame4.records.iter().any(|r| r.ordinal == 157));

    for frame in &fixture.frames {
        for record in &frame.records {
            assert_eq!(record.registers.len(), 16);
            assert!(record.stack.len >= 0x80);
            assert_eq!(record.stack.requested_len, 0x80);
            assert_eq!(record.stack.region, GlMemoryRegion::WorkRam);
            assert!(record.stack.bytes_hex.len() >= 0x80 * 2);
            assert_eq!(record.stack_words.len(), 0x80 / 4);
        }
    }

    let upload = frame2
        .records
        .iter()
        .find(|record| record.ordinal == 99)
        .expect("texture upload record");
    let source_word = upload
        .stack_words
        .iter()
        .find(|word| word.offset == 0x10)
        .expect("source pointer stack word");
    let source_snapshot = source_word
        .snapshot
        .as_ref()
        .expect("followed source pointer");
    assert_eq!(source_snapshot.region, GlMemoryRegion::WorkRam);
    let file = source_snapshot.file_backing.as_ref().expect("file backing");
    assert_eq!(file.path, "screenBG_565.pix");
    assert_eq!(file.offset, 70);
}
