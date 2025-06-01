use std::{env, u16};
use std::process::ExitCode;
use std::io::{self, BufRead, BufReader, Error, ErrorKind, BufWriter, Write};
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

impl Date {
    fn ge(&self, other: &Self) -> bool {
        if self.year > other.year { return true; }
        if self.year == other.year && self.month > other.month { return true; }
        if self.year == other.year && self.month == other.month && self.day >= other.day { return true; }
        false
    }

    fn le(&self, other: &Self) -> bool {
        if self.year < other.year { return true; }
        if self.year == other.year && self.month < other.month { return true; }
        if self.year == other.year && self.month == other.month && self.day <= other.day { return true; }
        false
    }
}

struct Doc {
    path: String,
    revdate: Option<Date>,
    content: String,
    has_imagesdir: bool,
}

fn usage() {
    eprintln!(
"usage: calendar_fast [-h|--help] [-v|--version] [-o <path>] [--header <path>] [--start-date <date>] [--end-date <date>] <src_path>
  -h, --help     print this and exit
  -v, --version  print version number
  -o             output path
  --header       header path (its contents will go to the beginning of the file)
  --footer       footer path (its contents will go to the end of the file)
  --start-date   start date (inclusive)
  --end-date     end date (inclusive)
");
}

fn version() {
   eprintln!("calendar_fast build 2025-06-01"); 
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

fn try_parse_date(date: &str) -> io::Result<Date> {
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
            return Err(error(format!("could not parse date '{}'", date)));
        }

        Ok(Date {year, month, day})
}

fn try_parse_date_with_prefix(line: &str, prefix: &'static str) -> io::Result<Option<Date>> {
    if let Some(date) = line.strip_prefix(prefix) {
        match try_parse_date(date) {
            Ok(d) => Ok(Some(d)),
            Err(e) => Err(e),
        }
    } else {
        Ok(None)
    }
}

static BOM: &'static str = unsafe { std::str::from_utf8_unchecked(&[0xEF, 0xBB, 0xBF]) };

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
        content: String::new(),
        has_imagesdir: false,
    };

    for (ln, line) in lines.enumerate() {
        if let Err(err) = line {
            return Err(error_with_file_and_line(path, ln, err));
        }
        let line = line?;
        let mut line_original = &line[..];
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

        let mut imagesdir: Option<String> = None;

        let comment = cmt_block || cmt_section;
        if !comment {
            if line.starts_with("include::") { return Ok(None); }

            if let None = doc.revdate {
                let revdate = try_parse_date_with_prefix(line, ":revdate: ");
                if let Err(err) = revdate {
                    return Err(error_with_file_and_line(path, ln, err));
                }
                if let Some(date) = revdate? {
                    doc.revdate = Some(date);
                }
            }

            let id = line.strip_prefix(":imagesdir: ");
            if let Some(id) = id {
                imagesdir = Some(id.to_string());
            }
        }

        if let Some(nb) = line_original.strip_prefix(BOM) {
            line_original = nb;
        }

        doc.content.push_str(&line_original);
        doc.content.push_str("\n");

        if let Some(dir) = imagesdir {
            doc.has_imagesdir = true;

            let p = Path::new(&dir);
            // HACK: unwrap
            // TODO: Actual is url
            if !p.has_root() && !p.starts_with("http://") && !p.starts_with("https://") {
                doc.content.push_str(":imagesdir: ");
                doc.content.push_str(&str::replace(path.parent().unwrap().join(p).to_str().unwrap(), "\\", "/"));
                doc.content.push_str("\n");
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

fn generate<'a>(path: &str, header: &str, footer: &str, docs: impl Iterator<Item = &'a Doc>) -> io::Result<()> {
    let file = File::create(path)?;
    let mut buf = BufWriter::new(file);

    
    buf.write(header.as_bytes())?;
    buf.write("\n\n:leveloffset: +1\n\n".as_bytes())?;
    
    for doc in docs {
        if !doc.has_imagesdir {
            let p = Path::new(&doc.path);
            // TODO: unwrap
            let parent = p.parent().unwrap().to_str().unwrap();
            let parent_fwd = str::replace(parent, "\\", "/");
            buf.write(format!(":imagesdir: {}\n", parent_fwd).as_bytes())?;
        }

        buf.write(doc.content.as_bytes())?;
        buf.write("\n\n".as_bytes())?;
    }

    buf.write("\n\n:leveloffset: -1\n\n".as_bytes())?;
    buf.write(footer.as_bytes())?;

    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args();
    args.next().unwrap();

    let mut src_dir: Option<String> = None;
    let mut out_path = String::from("calendar.adoc");
    let mut header_path: Option<String> = None;
    let mut footer_path: Option<String> = None;

    let mut start_date = Date { year: 0, month: 0, day: 0 };
    let mut end_date = Date { year: u16::MAX, month: u8::MAX, day: u8::MAX };

    while let Some(arg) = args.next() {
        // TODO: switch to match

        if arg == "-h" || arg == "--help" {
            usage();
            return ExitCode::SUCCESS;
        } else if arg == "-v" || arg == "--version" {
            version();
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
        } else if arg == "--start-date" {
            start_date = match try_parse_date(&args.next().unwrap()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(1);
                }
            }
        } else if arg == "--end-date" {
            end_date = match try_parse_date(&args.next().unwrap()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(1);
                }
            }
        } else if let Some(_) = src_dir {
            eprintln!("error: unexpected positional argument (multiple source directories are currently not supported)");
            return ExitCode::from(1);
        } else {
            src_dir = Some(arg);
        }
    }

    if let None = src_dir {
        usage();
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

    match generate(&out_path, &header, &footer, docs.iter().filter(|doc| {
        if let Some(date) = doc.revdate {
            date.ge(&start_date) && date.le(&end_date)
        } else {
            false
        }
    })) {
        Ok(_) => {},
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(1);
        }
    };

    ExitCode::SUCCESS
}
