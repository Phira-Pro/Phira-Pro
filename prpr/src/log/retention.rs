use super::{LOG_FILE_FORMAT, LOG_FILE_PREFIX, LOG_KEEP_DAYS};
use chrono::{Duration, NaiveDate, Utc};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

const SEGMENT_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct LogFile {
    pub(super) date: NaiveDate,
    sequence: u32,
    pub(super) path: PathBuf,
    size: u64,
}

impl LogFile {
    fn parse(name: &str) -> Option<(NaiveDate, u32)> {
        let stem = name.strip_prefix(LOG_FILE_PREFIX)?.strip_prefix('.')?.strip_suffix(".log")?;
        let (date, sequence) = match stem.split_once('.') {
            Some((date, sequence)) if sequence.len() == 10 && sequence.bytes().all(|byte| byte.is_ascii_digit()) => (date, sequence.parse().ok()?),
            Some(_) => return None,
            None => (stem, 0),
        };
        if date.len() != 10 {
            return None;
        }
        Some((NaiveDate::parse_from_str(date, LOG_FILE_FORMAT).ok()?, sequence))
    }
}

pub(super) fn log_files(dir: &Path) -> io::Result<Vec<LogFile>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some((date, sequence)) = name.to_str().and_then(LogFile::parse) else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        match entry.metadata() {
            Ok(metadata) => files.push(LogFile {
                date,
                sequence,
                path: entry.path(),
                size: metadata.len(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    files.sort_unstable_by_key(|file| (file.date, file.sequence));
    Ok(files)
}

struct ActiveFile {
    date: NaiveDate,
    path: PathBuf,
    size: u64,
    file: File,
}

pub(super) struct LogFiles {
    dir: PathBuf,
    limit: u64,
    total: u64,
    active: Option<ActiveFile>,
}

impl LogFiles {
    pub(super) fn new(dir: PathBuf, limit: u64) -> Self {
        let mut files = Self {
            dir,
            limit,
            total: 0,
            active: None,
        };
        if let Err(error) = files.prune(Utc::now().date_naive(), 0) {
            eprintln!("failed to clean up logs: {error}");
        }
        files
    }

    pub(super) fn set_limit(&mut self, limit: u64) -> io::Result<()> {
        self.limit = limit;
        if self.active.as_ref().is_some_and(|active| active.size > limit) {
            self.active = None;
        }
        self.prune(Utc::now().date_naive(), 0)?;
        Ok(())
    }

    fn prune(&mut self, today: NaiveDate, incoming: u64) -> io::Result<u32> {
        let files = log_files(&self.dir)?;
        let next = files
            .iter()
            .filter(|file| file.date == today)
            .map(|file| file.sequence)
            .max()
            .map_or(0, |sequence| sequence.saturating_add(1));
        // Windows directory entries can report an old length for an open file.
        // Account for the active segment using the writer's byte count.
        self.total = self.active.as_ref().map_or(0, |active| active.size)
            + files
                .iter()
                .filter(|file| self.active.as_ref().is_none_or(|active| active.path != file.path))
                .map(|file| file.size)
                .sum::<u64>();
        let oldest = today - Duration::days(LOG_KEEP_DAYS as i64 - 1);
        for file in files {
            if self.active.as_ref().is_some_and(|active| active.path == file.path) {
                continue;
            }
            if file.date < oldest || self.total.saturating_add(incoming) > self.limit {
                match fs::remove_file(&file.path) {
                    Ok(()) => self.total -= file.size,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => self.total -= file.size,
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(next)
    }

    pub(super) fn append(&mut self, line: &str) -> io::Result<()> {
        let today = Utc::now().date_naive();
        let max_segment = SEGMENT_BYTES.min(self.limit);
        let clipped = line.len() as u64 > max_segment;
        let line = if clipped {
            &line[..line.floor_char_boundary((max_segment - 1) as usize)]
        } else {
            line
        };
        let size = line.len() as u64 + u64::from(clipped);
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.date != today || active.size + size > max_segment)
        {
            self.active = None;
        }
        if self.active.is_none() || self.total.saturating_add(size) > self.limit {
            let next = self.prune(today, size)?;
            if self.active.is_none() {
                self.open(today, next)?;
            }
        }
        let active = self.active.as_mut().unwrap();
        let result = active
            .file
            .write_all(line.as_bytes())
            .and_then(|()| if clipped { active.file.write_all(b"\n") } else { Ok(()) });
        if result.is_ok() {
            active.size += size;
            self.total += size;
        } else {
            self.active = None;
        }
        result
    }

    fn open(&mut self, date: NaiveDate, sequence: u32) -> io::Result<()> {
        let date_label = date.format(LOG_FILE_FORMAT);
        for sequence in sequence..=u32::MAX {
            let name = if sequence == 0 {
                format!("{LOG_FILE_PREFIX}.{date_label}.log")
            } else {
                format!("{LOG_FILE_PREFIX}.{date_label}.{sequence:010}.log")
            };
            let path = self.dir.join(name);
            match File::create_new(&path) {
                Ok(file) => {
                    self.active = Some(ActiveFile { date, path, size: 0, file });
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("no available log segment"))
    }

    pub(super) fn flush(&mut self) {
        if let Some(active) = &mut self.active {
            let _ = active.file.flush();
        }
    }
}
