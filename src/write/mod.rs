// Copyright (c) 2021 Harry [Majored] [hello@majored.pw]
// MIT License (https://github.com/Majored/rs-async-zip/blob/main/LICENSE)

//! A module which supports writing ZIP files.
//!
//! # Example
//! ### Whole data (u8 slice)
//! ```no_run
//! # use async_zip::{Compression, write::{EntryOptions, ZipFileWriter}};
//! # use tokio::{fs::File, io::AsyncWriteExt};
//! # use async_zip::error::ZipError;
//! #
//! # async fn run() -> Result<(), ZipError> {
//! let mut file = File::create("foo.zip").await?;
//! let mut writer = ZipFileWriter::new(&mut file);
//!
//! let data = b"This is an example file.";
//! let opts = EntryOptions::new(String::from("foo.txt"), Compression::Deflate);
//!
//! writer.write_entry_whole(opts, data).await?;
//! writer.close().await?;
//! #   Ok(())
//! # }
//! ```
//! ### Stream data (unknown size & data)
//! ```no_run
//! # use async_zip::{Compression, write::{EntryOptions, ZipFileWriter}};
//! # use tokio::{fs::File, io::AsyncWriteExt};
//! # use async_zip::error::ZipError;
//! #
//! # async fn run() -> Result<(), ZipError> {
//! let mut file = File::create("foo.zip").await?;
//! let mut writer = ZipFileWriter::new(&mut file);
//!
//! let data = b"This is an example file.";
//! let opts = EntryOptions::new(String::from("bar.txt"), Compression::Deflate);
//!
//! let mut entry_writer = writer.write_entry_stream(opts).await?;
//! entry_writer.write_all(data).await.unwrap();
//!
//! entry_writer.close().await?;
//! writer.close().await?;
//! #   Ok(())
//! # }
//! ```

pub(crate) mod compressed_writer;
pub(crate) mod entry_stream;
pub(crate) mod entry_whole;

pub use entry_stream::EntryStreamWriter;

use crate::error::Result;
use crate::spec::compression::Compression;
use crate::spec::header::{CentralDirectoryHeader, EndOfCentralDirectoryHeader};
use entry_whole::EntryWholeWriter;
use async_io_utilities::AsyncOffsetWriter;

use tokio::io::{AsyncWrite, AsyncWriteExt};

/// A set of options for opening new ZIP entries.
pub struct EntryOptions {
    pub(crate) filename: String,
    pub(crate) compression: Compression,
    extra: Vec<u8>,
    comment: String,
    unix_permissions: u32,
}

impl EntryOptions {
    /// Construct a new set of options from its required constituents.
    pub fn new(filename: String, compression: Compression) -> Self {
        EntryOptions { 
            filename, 
            compression,
            extra: Vec::new(),
            comment: String::new(),
            unix_permissions: 0,
        }
    }

    /// Consume the options and override the extra field data.
    pub fn extra(mut self, extra: Vec<u8>) -> Self {
        self.extra = extra;
        self
    }

    /// Consume the options and override the file comment.
    pub fn comment(mut self, comment: String) -> Self {
        self.comment = comment;
        self
    }

    /// Consume unix permissions option for zip files (ex. 0o755)
    pub fn unix_permissions(mut self, unix_permissions: u32) -> Self {
        self.unix_permissions = unix_permissions;
        self
    }
}

pub(crate) struct CentralDirectoryEntry {
    pub header: CentralDirectoryHeader,
    pub opts: EntryOptions,
}

/// A ZIP file writer which acts over AsyncWrite implementers.
///
/// # Note
/// - [`ZipFileWriter::close()`] must be called before a stream writer goes out of scope.
pub struct ZipFileWriter<W: AsyncWrite + Unpin> {
    pub(crate) writer: AsyncOffsetWriter<W>,
    pub(crate) cd_entries: Vec<CentralDirectoryEntry>,
    comment_opt: Option<String>,
}

impl<W: AsyncWrite + Unpin> ZipFileWriter<W> {
    /// Construct a new ZIP file writer from a mutable reference to a writer.
    pub fn new(writer: W) -> Self {
        Self { writer: AsyncOffsetWriter::new(writer), cd_entries: Vec::new(), comment_opt: None }
    }

    /// Write a new ZIP entry of known size and data.
    pub async fn write_entry_whole(&mut self, options: EntryOptions, data: &[u8]) -> Result<()> {
        EntryWholeWriter::from_raw(self, options, data).write().await
    }

    /// Write an entry of unknown size and data via streaming (ie. using a data descriptor).
    pub async fn write_entry_stream(&mut self, options: EntryOptions) -> Result<EntryStreamWriter<'_, W>> {
        EntryStreamWriter::from_raw(self, options).await
    }

    /// Set the ZIP file comment.
    pub fn comment(&mut self, comment: String) {
        self.comment_opt = Some(comment);
    }

    /// Consumes this ZIP writer and completes all closing tasks.
    ///
    /// This includes:
    /// - Writing all central directroy headers.
    /// - Writing the end of central directory header.
    /// - Writing the file comment.
    ///
    /// Failiure to call this function before going out of scope would result in a corrupted ZIP file.
    pub async fn close(mut self) -> Result<()> {
        let cd_offset = self.writer.offset();

        for entry in &self.cd_entries {
            self.writer.write_all(&crate::spec::signature::CENTRAL_DIRECTORY_FILE_HEADER.to_le_bytes()).await?;
            self.writer.write_all(&entry.header.as_slice()).await?;
            self.writer.write_all(entry.opts.filename.as_bytes()).await?;
            self.writer.write_all(&entry.opts.extra).await?;
            self.writer.write_all(entry.opts.comment.as_bytes()).await?;
        }

        let header = EndOfCentralDirectoryHeader {
            disk_num: 0,
            start_cent_dir_disk: 0,
            num_of_entries_disk: self.cd_entries.len() as u16,
            num_of_entries: self.cd_entries.len() as u16,
            size_cent_dir: (self.writer.offset() - cd_offset) as u32,
            cent_dir_offset: cd_offset as u32,
            file_comm_length: self.comment_opt.as_ref().map(|v| v.len() as u16).unwrap_or_default(),
        };

        self.writer.write_all(&crate::spec::signature::END_OF_CENTRAL_DIRECTORY.to_le_bytes()).await?;
        self.writer.write_all(&header.as_slice()).await?;
        if let Some(comment) = self.comment_opt {
            self.writer.write_all(comment.as_bytes()).await?;
        }

        Ok(())
    }
}
