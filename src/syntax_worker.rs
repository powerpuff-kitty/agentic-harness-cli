//! Isolate parser/visitor stack exhaustion from the reporting process. One native
//! worker is reused across files; a failed worker is replaced for the next file.
#![cfg_attr(test, allow(dead_code))] // Unit tests use the parser directly; CLI tests cover workers.
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

pub(crate) type Parsed = (Vec<(usize, String, String)>, Vec<usize>, Vec<String>);
const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_SOURCE: usize = 2_000_000;
static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

fn read_frame(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::other("parser frame exceeds limit"));
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}
fn write_frame(output: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_FRAME {
        return Err(io::Error::other("parser frame exceeds limit"));
    }
    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
    output.write_all(bytes)?;
    output.flush()
}
struct Worker {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
impl Worker {
    fn start() -> io::Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--internal-syntax-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            input: child.stdin.take().unwrap(),
            output: child.stdout.take().unwrap(),
            child,
        })
    }
    fn analyze(&mut self, path: &Path, text: &str) -> io::Result<Parsed> {
        let request = serde_json::to_vec(&(path.to_string_lossy(), text))?;
        write_frame(&mut self.input, &request)?;
        Ok(serde_json::from_slice(&read_frame(&mut self.output)?)?)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub fn analyze(path: &Path, text: &str) -> Parsed {
    if text.len() > MAX_SOURCE {
        return (Vec::new(), vec![1], Vec::new());
    }
    let mut worker = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    if worker.is_none() {
        *worker = Worker::start().ok();
    }
    if let Some(worker) = worker.as_mut()
        && let Ok(result) = worker.analyze(path, text)
    {
        return result;
    }
    // A crashed parser yields incomplete evidence, never a successful empty graph.
    *worker = None;
    (Vec::new(), vec![1], Vec::new())
}
pub fn run() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let bytes = match read_frame(&mut input) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        let (path, source): (String, String) = serde_json::from_slice(&bytes)?;
        if source.len() > MAX_SOURCE {
            return Err(io::Error::other("parser source exceeds limit"));
        }
        let (visitor, gaps) = crate::syntax::analyze(Path::new(&path), &source);
        write_frame(
            &mut output,
            &serde_json::to_vec(&(visitor.imports, gaps, visitor.tags))?,
        )?;
    }
}

pub fn shutdown() {
    let mut worker = WORKER.lock().unwrap_or_else(|e| e.into_inner());
    *worker = None;
}
