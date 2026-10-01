use std::{
    fs,
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
};

use crate::{error::DiagnosticCode, runtime::files};

pub(super) struct CheckpointError {
    pub(super) code: DiagnosticCode,
    pub(super) message: String,
}

type CheckpointResult<T> = Result<T, CheckpointError>;

fn io_error(message: impl Into<String>) -> CheckpointError {
    CheckpointError {
        code: DiagnosticCode::RuntimeIo,
        message: message.into(),
    }
}

fn state_error(message: impl Into<String>) -> CheckpointError {
    CheckpointError {
        code: DiagnosticCode::RuntimeGeneral,
        message: message.into(),
    }
}

fn argument_error(message: impl Into<String>) -> CheckpointError {
    CheckpointError {
        code: DiagnosticCode::RuntimeArgument,
        message: message.into(),
    }
}

pub(super) struct CheckpointState {
    pub(super) position: usize,
    pub(super) output_len: u64,
    pub(super) output_checksum: u64,
    pub(super) input_path: String,
    pub(super) output_path: String,
    pub(super) input_len: u64,
    pub(super) input_modified_ns: u128,
}

fn modified_time_ns(path: &str) -> CheckpointResult<u128> {
    let modified = fs::metadata(path)
        .map_err(|error| {
            io_error(format!(
                "could not inspect checkpoint input `{path}`: {error}"
            ))
        })?
        .modified()
        .map_err(|error| {
            io_error(format!(
                "could not read modification time for `{path}`: {error}"
            ))
        })?;
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| {
            state_error(format!(
                "checkpoint input `{path}` has an invalid modification time"
            ))
        })
}

fn output_prefix_checksum(path: &str, length: u64) -> CheckpointResult<u64> {
    let file = fs::File::open(path).map_err(|error| {
        io_error(format!(
            "could not read checkpoint output `{path}`: {error}"
        ))
    })?;
    let mut reader = BufReader::new(file.take(length));
    let mut checksum = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader.read(&mut buffer).map_err(|error| {
            io_error(format!(
                "could not read checkpoint output `{path}`: {error}"
            ))
        })?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok(checksum)
}

fn encode_checkpoint_path(path: &str) -> String {
    path.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_checkpoint_path(encoded: &str) -> CheckpointResult<String> {
    if !encoded.len().is_multiple_of(2) {
        return Err(state_error("invalid encoded checkpoint path"));
    }
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| state_error("invalid encoded checkpoint path"))?;
    String::from_utf8(bytes).map_err(|_| state_error("checkpoint path is not valid UTF-8"))
}

pub(super) fn read_checkpoint(path: &str) -> CheckpointResult<Option<CheckpointState>> {
    match fs::read_to_string(path) {
        Ok(value) => {
            let invalid = || {
                state_error(format!(
                    "checkpoint `{path}` contains invalid recovery state"
                ))
            };
            let mut fields = value.lines();
            let position = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<usize>()
                .map_err(|_| invalid())?;
            let output_len = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            let output_checksum = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            let input_len = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u64>()
                .map_err(|_| invalid())?;
            let input_modified_ns = fields
                .next()
                .ok_or_else(invalid)?
                .parse::<u128>()
                .map_err(|_| invalid())?;
            let input_path = decode_checkpoint_path(fields.next().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            let output_path = decode_checkpoint_path(fields.next().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            if fields.next().is_some() {
                return Err(invalid());
            }
            Ok(Some(CheckpointState {
                position,
                output_len,
                output_checksum,
                input_path,
                output_path,
                input_len,
                input_modified_ns,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(format!(
            "could not read checkpoint `{path}`: {error}"
        ))),
    }
}

fn write_checkpoint(path: &str, state: &CheckpointState) -> CheckpointResult<()> {
    let contents = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        state.position,
        state.output_len,
        state.output_checksum,
        state.input_len,
        state.input_modified_ns,
        encode_checkpoint_path(&state.input_path),
        encode_checkpoint_path(&state.output_path),
    );
    files::atomic_write(Path::new(path), contents.as_bytes())
        .map_err(|error| io_error(format!("could not write checkpoint `{path}`: {error}")))
}

pub(super) fn checkpoint_progress(
    path: Option<&str>,
    processed: usize,
    interval: usize,
    output: Option<&mut fs::File>,
    input_path: &str,
    output_path: Option<&str>,
) -> CheckpointResult<()> {
    if let Some(path) = path
        && processed.is_multiple_of(interval)
    {
        let output_len = if let Some(output) = output {
            output
                .flush()
                .map_err(|error| io_error(format!("could not flush checkpoint output: {error}")))?;
            output
                .metadata()
                .map_err(|error| io_error(format!("could not inspect checkpoint output: {error}")))?
                .len()
        } else {
            return Err(argument_error("checkpoint requires a CSV output sink"));
        };
        let output_path =
            output_path.ok_or_else(|| argument_error("checkpoint requires an output path"))?;
        let output_checksum = output_prefix_checksum(output_path, output_len)?;
        let input_metadata = fs::metadata(input_path).map_err(|error| {
            io_error(format!(
                "could not inspect checkpoint input `{input_path}`: {error}"
            ))
        })?;
        write_checkpoint(
            path,
            &CheckpointState {
                position: processed,
                output_len,
                output_checksum,
                input_path: input_path.into(),
                output_path: output_path.into(),
                input_len: input_metadata.len(),
                input_modified_ns: modified_time_ns(input_path)?,
            },
        )?;
    }
    Ok(())
}

pub(super) fn remove_checkpoint(path: &str) -> CheckpointResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(format!(
            "could not remove checkpoint `{path}`: {error}"
        ))),
    }
}

pub(super) fn validate_checkpoint_source(
    state: &CheckpointState,
    input_path: &str,
    output_path: &str,
) -> CheckpointResult<()> {
    if state.input_path != input_path || state.output_path != output_path {
        return Err(argument_error(
            "checkpoint input or output path does not match this Flow",
        ));
    }
    let input_len = fs::metadata(input_path)
        .map_err(|error| {
            io_error(format!(
                "could not inspect checkpoint input `{input_path}`: {error}"
            ))
        })?
        .len();
    if state.input_len != input_len || state.input_modified_ns != modified_time_ns(input_path)? {
        return Err(state_error(format!(
            "checkpoint input `{input_path}` changed since the checkpoint was written"
        )));
    }
    Ok(())
}

pub(super) fn validate_checkpoint_output(
    state: &CheckpointState,
    output_path: &str,
) -> CheckpointResult<()> {
    let output_len = fs::metadata(output_path)
        .map_err(|error| {
            io_error(format!(
                "could not inspect checkpoint output `{output_path}`: {error}"
            ))
        })?
        .len();
    if state.output_len > output_len {
        return Err(state_error(format!(
            "checkpoint output length exceeds the current output file `{output_path}`"
        )));
    }
    if output_prefix_checksum(output_path, state.output_len)? != state.output_checksum {
        return Err(state_error(format!(
            "checkpoint output `{output_path}` changed since the checkpoint was written"
        )));
    }
    Ok(())
}

fn resolved_path(path: &str) -> CheckpointResult<PathBuf> {
    let path = Path::new(path);
    if let Ok(resolved) = fs::canonicalize(path) {
        return Ok(resolved);
    }
    let name = path
        .file_name()
        .ok_or_else(|| argument_error(format!("path `{}` has no file name", path.display())))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| {
        io_error(format!(
            "could not resolve directory `{}`: {error}",
            parent.display()
        ))
    })?;
    Ok(parent.join(name))
}

pub(super) fn paths_are_same(left: &str, right: &str) -> CheckpointResult<bool> {
    Ok(resolved_path(left)? == resolved_path(right)?)
}
