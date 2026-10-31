//! Small persistent Wayland owner, independent of GTK and the preview window.
use anyhow::{Context, Result, bail, ensure};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};
use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

fn payloads(uris: &[String]) -> Result<Vec<(&'static str, Vec<u8>)>> {
    ensure!(!uris.is_empty() && uris.len() <= 4096, "需要本机文件 URI");
    ensure!(
        uris.iter().all(|s| s.starts_with("file://")
            && s.len() <= 16 * 1024
            && !s.contains(['\r', '\n', '\0'])),
        "无效文件 URI"
    );
    Ok(vec![
        ("text/uri-list", (uris.join("\r\n") + "\r\n").into_bytes()),
        (
            "x-special/gnome-copied-files",
            ("copy\n".to_string() + &uris.join("\n")).into_bytes(),
        ),
        ("application/x-kde-cutselection", b"0".to_vec()),
    ])
}
fn serve(uris: &[String]) -> Result<()> {
    let sources = payloads(uris)?
        .into_iter()
        .map(|(mime, bytes)| MimeSource {
            source: Source::Bytes(bytes.into()),
            mime_type: MimeType::Specific(mime.into()),
        })
        .collect();
    let mut options = Options::new();
    options
        .foreground(true)
        .omit_additional_text_mime_types(true);
    let prepared = options.prepare_copy_multi(sources)?;
    println!("ready");
    std::io::stdout().flush()?;
    prepared.serve()?;
    Ok(())
}
fn run() -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|s| s == "--serve") {
        args.remove(0);
        return serve(&args);
    }
    let sources = payloads(&args)?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--serve")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let output = child.stdout.take().context("缺少剪贴板响应")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(output).read_line(&mut line);
        let _ = sender.send(line);
    });
    if receiver
        .recv_timeout(Duration::from_secs(2))
        .ok()
        .is_some_and(|s| s == "ready\n")
    {
        // This parent exits. The small serving child lives until the compositor
        // replaces the selection, so closing the viewer does not lose the file.
        return Ok(());
    }
    let _ = child.kill();
    let _ = child.wait();
    // GNOME compositors without a data-control protocol can still use the
    // existing wl-copy surface-based fallback for the GNOME file target.
    let data = &sources[1].1;
    let mut fallback = Command::new("wl-copy")
        .args(["--type", "x-special/gnome-copied-files"])
        .stdin(Stdio::piped())
        .spawn()
        .context("此合成器需要 wl-copy 剪贴板后端")?;
    fallback
        .stdin
        .take()
        .context("无法写入剪贴板")?
        .write_all(data)?;
    if !fallback.wait()?.success() {
        bail!("剪贴板写入失败");
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("quicklook-clipboard: {error:#}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_formats_match_gnome_and_kde_contracts() {
        let uri = "file:///tmp/%E4%B8%AD%E6%96%87%0A%23.txt".to_string();
        let formats = payloads(std::slice::from_ref(&uri)).unwrap();
        assert_eq!(formats[0].1, format!("{uri}\r\n").as_bytes());
        assert_eq!(formats[1].1, format!("copy\n{uri}").as_bytes());
        assert_eq!(formats[2].1, b"0");
        assert!(payloads(&["file:///tmp/x\nfile:///etc/passwd".into()]).is_err());
    }
}
