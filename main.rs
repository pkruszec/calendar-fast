use std::env;
use std::process::ExitCode;
use std::io::{self, BufRead, BufReader, Error, ErrorKind, BufWriter, Write, Read};
use std::fs::{self, File};
use std::path::Path;
use std::cmp::Ordering;

#[derive(Clone, Copy)]
#[derive(Debug)]
struct Date {
    year: u16,
    month: u8,
    day: u8,
}

struct Doc {
    path: String,
    revdate: Option<Date>,
    imagesdir: Option<String>,
}

fn usage(prog: &str) {
    eprintln!(
"usage: {} [flags] <src-dir>
flags available:
  -h, --help  Show the usage and exit
  -o          Output path
  --header    Header path
  --footer    Footer path
",
    prog);
}

fn error(text: String) -> Error {
    Error::new(ErrorKind::Other, text)
}

fn error_with_file(path: &Path, err: Error) -> Error {
    Error::new(ErrorKind::Other, format!("{}: {}", path.display(), err))
}

fn error_with_file_and_line(path: &Path, line: usize, err: Error) -> Error {
    Error::new(ErrorKind::Other, format!("{}:{}: {}", path.display(), line + 1, err))
}

fn try_parse_date(line: &str, prefix: &'static str) -> io::Result<Option<Date>> {
    if let Some(date) = line.strip_prefix(prefix) {
        let len = 4 + 1 + 2 + 1 + 2;
        let mut ok = date.len() == len;

        let mut year = 0u16;
        let mut month = 0u8;
        let mut day = 0u8;

        if ok {
            let date = date.as_bytes();
            ok = date[4] == b'-' && date[7] == b'-';
        }

        if ok {
            year = date[0..=3].parse().unwrap_or_else(|_| { ok = false; 0 });
            month = date[5..=6].parse().unwrap_or_else(|_| { ok = false; 0 });
            day = date[8..=9].parse().unwrap_or_else(|_| { ok = false; 0 });

            ok = year > 0 && month >= 1 && month <= 12 && day >= 1 && day <= 31;
        }

        if !ok {
            return Err(error(format!("Could not parse date '{}'", date)));
        }

        Ok(Some(Date {year, month, day}))
    } else {
        Ok(None)
    }
}

fn get_doc(path: &Path) -> io::Result<Option<Doc>> {
    let file = File::open(path);
    if let Err(err) = file {
        return Err(error_with_file(path, err));
    }
    let file = file?;
    let lines = BufReader::new(file).lines();

    let mut cmt_block = false;
    let mut cmt_section = false;
    let mut cmt_section_block = false;

    let mut doc = Doc {
        path: path.to_string_lossy().to_string(),
        revdate: None,
        imagesdir: None,
    };

    for (ln, line) in lines.enumerate() {
        if let Err(err) = line {
            return Err(error_with_file_and_line(path, ln, err));
        }
        let line = line?;
        let line = line.trim();

        if line == "////" {
            cmt_block = !cmt_block;
        } else if line == "[comment]" {
            cmt_section = true;
        } else if cmt_section {
            if line == "--" {
                if !cmt_section_block {
                    cmt_section_block = true;
                } else {
                    cmt_section_block = false;
                    cmt_section = false;
                }
            } else if line == "" {
                if !cmt_section_block {
                    cmt_section = false
                }
            }
        }

        let comment = cmt_block || cmt_section;
        if !comment {
            if line.starts_with("include::") { return Ok(None); }

            if let None = doc.revdate {
                let revdate = try_parse_date(line, ":revdate: ");
                if let Err(err) = revdate {
                    return Err(error_with_file_and_line(path, ln, err));
                }
                if let Some(date) = revdate? {
                    doc.revdate = Some(date);
                }
            }

            if let None = doc.imagesdir {
                let line_no_prefix = line.strip_prefix(":imagesdir: ");
                if let Some(line_no_prefix) = line_no_prefix {
                    doc.imagesdir = Some(line_no_prefix.to_owned());
                }
            }
        }
    }

    Ok(Some(doc))
}

fn traverse(path: &Path, out: &mut Vec<Doc>) -> io::Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            traverse(&path, out)?;
        }
    } else if path.is_file() {

        let ext = path.extension();
        if ext.is_none() {
            return Ok(());
        } else if let Some(ext) = ext {
            if ext.to_str() != Some("adoc") {
                return Ok(());
            }
        }

        let doc = get_doc(path)?;
        if let Some(doc) = doc {
            out.push(doc);
        }
    }
    Ok(())
}

fn write_contents<W: Write>(path: &str, buf: &mut BufWriter<W>) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // We read chunks of input and write it to the output.
    // We could just read it all into a string, but that would require allocations.
    let mut tmp = [0u8; 256];

    loop {
        let read = reader.read(&mut tmp)?;

        // If we find BOM at the beginning of the buffer, we trim it.
        // This has a side effect - some BOMs are removed, even if they are not at the beginning of the file.
        // It may be fine, but it requires some testing regarding how Asciidoctor handles that.
        // (BOMs after the beginning of the file should probably not happen).
        const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

        if read >= BOM.len() && tmp[..BOM.len()] == BOM {
            buf.write(&tmp[BOM.len()..read])?;
        } else {
            buf.write(&tmp[..read])?;
        }

        if read < tmp.len() { break; }
    }

    Ok(())
}

fn generate<'a>(path: &str, header: &str, footer: &str, docs: impl Iterator<Item = &'a Doc>) -> io::Result<()> {
    let file = File::create(path)?;
    let mut buf = BufWriter::new(file);

    
    buf.write(header.as_bytes())?;
    buf.write("\n\n:leveloffset: +1\n\n".as_bytes())?;
    
    for doc in docs {
        // HACK
        if let None = &doc.imagesdir {
            let p = Path::new(&doc.path);
            // TODO: unwrap
            let parent = p.parent().unwrap().to_str().unwrap();
            let parent_fwd = str::replace(parent, "\\", "/");
            buf.write(format!(":imagesdir: {}\n", parent_fwd).as_bytes())?;
        }

        write_contents(&doc.path, &mut buf)?;
        buf.write("\n\n".as_bytes())?;
    }

    buf.write("\n\n:leveloffset: -1\n\n".as_bytes())?;
    buf.write(footer.as_bytes())?;

    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args();
    let prog = args.next().unwrap();

    let mut src_dir: Option<String> = None;
    let mut out_path = String::from("calendar.adoc");
    let mut header_path: Option<String> = None;
    let mut footer_path: Option<String> = None;

    while let Some(arg) = args.next() {
        if arg == "-h" || arg == "--help" {
            usage(&prog);
            return ExitCode::SUCCESS;
        } else if arg == "--header" {
            // TODO: good error message
            header_path = Some(args.next().unwrap());
        } else if arg == "--footer" {
            // TODO: good error message
            footer_path = Some(args.next().unwrap());
        } else if arg == "-o" {
            // TODO: good error message
            out_path = args.next().unwrap();
        } else if let Some(_) = src_dir {
            eprintln!("error: unexpected positional argument (multiple source directories are currently not supported)");
            return ExitCode::from(1);
        } else {
            src_dir = Some(arg);
        }
    }

    if let None = src_dir {
        usage(&prog);
        eprintln!("error: source directory not provided");
        return ExitCode::from(1);
    }

    let src_dir = src_dir.unwrap();
    let src_path = &Path::new(&src_dir);

    if !src_path.exists() {
        eprintln!("error: source directory does not exist");
        return ExitCode::from(1);
    }

    // TODO: unwrap

    let header = if let Some(path) = header_path {
        fs::read_to_string(path).unwrap()
    } else {
        String::from("= Calendar\n\n")
    };

    let footer = if let Some(path) = footer_path {
        fs::read_to_string(path).unwrap()
    } else {
        String::from("")
    };

    let mut docs: Vec<Doc> = Vec::new();

    match traverse(src_path, &mut docs) {
        Ok(_) => {},
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };

    docs.sort_by(|a, b| {
        // Sort by revdates in descending order (newest on the top).

        let l = a.revdate;
        let r = b.revdate;

        // TODO: Make it more concise
        if l.is_none() && r.is_none() {
            return Ordering::Equal;
        } else if l.is_none() {
            return Ordering::Greater;
        } else if r.is_none() {
            return Ordering::Less;
        }

        let l = l.unwrap();
        let r = r.unwrap();

        let y = r.year.cmp(&l.year);
        let m = r.month.cmp(&l.month);
        let d = r.day.cmp(&l.day);

        if y != Ordering::Equal { return y; }
        if m != Ordering::Equal { return m; }
        if d != Ordering::Equal { return d; }

        Ordering::Equal
    });

    match generate(&out_path, &header, &footer, docs.iter()) {
        Ok(_) => {},
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };

    ExitCode::SUCCESS
}
