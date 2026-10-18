use super::*;

struct FaultStorage {
    bytes: Vec<u8>,
    remaining: Option<usize>,
    flush_fails: bool,
    sync_fails: bool,
    rollback_fails: bool,
}

impl FaultStorage {
    fn healthy() -> Self {
        Self { bytes: b"committed\n".to_vec(), remaining: None, flush_fails: false,
            sync_fails: false, rollback_fails: false }
    }
}

impl Write for FaultStorage {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = match self.remaining {
            Some(0) => return Err(io::Error::other("injected write failure")),
            Some(left) => left.min(bytes.len()),
            None => bytes.len(),
        };
        self.bytes.extend_from_slice(&bytes[..n]);
        if let Some(left) = &mut self.remaining { *left -= n; }
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.flush_fails { Err(io::Error::other("injected flush failure")) } else { Ok(()) }
    }
}

impl JournalStorage for FaultStorage {
    fn journal_len(&self) -> io::Result<u64> { Ok(self.bytes.len() as u64) }
    fn rollback(&mut self, len: u64) -> io::Result<()> {
        if self.rollback_fails { return Err(io::Error::other("injected rollback failure")); }
        self.bytes.truncate(len as usize);
        Ok(())
    }
    fn sync_journal(&mut self) -> io::Result<()> {
        if self.sync_fails { Err(io::Error::other("injected sync failure")) } else { Ok(()) }
    }
}

#[test]
fn partial_write_and_flush_sync_failures_rollback_before_retry() {
    for failure in 0..3 {
        let mut sink = FaultStorage::healthy();
        if failure == 0 { sink.remaining = Some(4); }
        if failure == 1 { sink.flush_fails = true; }
        if failure == 2 { sink.sync_fails = true; }
        let error = append_bytes(&mut sink, b"new-entry\n").expect_err("injected failure");
        assert!(!error.rollback_failed);
        assert_eq!(sink.bytes, b"committed\n", "failed append must leave the committed prefix exact");
        sink.remaining = None;
        sink.flush_fails = false;
        sink.sync_fails = false;
        append_bytes(&mut sink, b"new-entry\n").expect("retry after recovery");
        assert_eq!(sink.bytes, b"committed\nnew-entry\n", "no duplicated prefix or partial JSON after recovery");
    }
}

#[test]
fn rollback_failure_is_explicit_and_cannot_be_treated_as_retryable() {
    let mut sink = FaultStorage::healthy();
    sink.remaining = Some(4);
    sink.rollback_fails = true;
    let error = append_bytes(&mut sink, b"new-entry\n").expect_err("write and rollback fail");
    assert!(error.rollback_failed);
    assert_eq!(sink.bytes, b"committed\nnew-");
}

#[test]
fn real_file_opened_like_append_journal_can_rollback_its_written_prefix() {
    let path = std::env::temp_dir().join(format!("n7rt-journal-file-rollback-{}.ndjson", std::process::id()));
    let original = b"committed\n";
    fs::write(&path, original).unwrap();
    let mut file = OpenOptions::new().create(true).read(true).write(true).open(&path).unwrap();
    file.seek(SeekFrom::End(0)).unwrap();
    let before = file.journal_len().unwrap();
    file.write_all(b"partial-new-entry").unwrap();
    file.flush().unwrap();
    let rolled_back = file.rollback(before);
    drop(file);
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    rolled_back.expect("the real journal handle must have permission to truncate on this OS");
    assert_eq!(bytes, original);
}
