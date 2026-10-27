#[derive(Clone, Debug)]
pub struct Options {
    pub child: bool,
    pub parent: bool,
    pub anim: bool,
    pub memtree: bool,
    pub gspawn: bool,
    pub reset: bool,
    pub frametime: bool,
    pub simple: bool,
    pub verbose: bool,
    pub interval: u64,
    pub dump: Option<char>,
    pub samples: Option<u64>,
    pub pid: Option<u32>,
    pub plain: bool,
}

pub const HELP: &str = "T6 Error Tracker (Rust)\n\
Usage: t6-error-tracker [options]\n\
With no metric flags all metrics are tracked. Attach to a local T6 Zombies host.\n\n\
  -c  child variables        -p  parent variables\n\
  -a  animation info         -m  script memory tree\n\
  -g  server entities        -r  server snapshot reset\n\
  -f  com_frameTime          -s  compact display\n\
  -v  all memory tree nodes  -t MILLISECONDS  refresh interval (default 50)\n\
  -d e|a                    dump entities or animations to dumps/\n\
  --pid PID                 attach to a specific process\n\
  --samples COUNT           exit after COUNT live samples\n\
  --once                    equivalent to --samples 1\n\
  --plain                   append samples without terminal cursor control\n\
  -h, --help                show this help\n";

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut o = Self {
            child: false,
            parent: false,
            anim: false,
            memtree: false,
            gspawn: false,
            reset: false,
            frametime: false,
            simple: false,
            verbose: false,
            interval: 50,
            dump: None,
            samples: None,
            pid: None,
            plain: false,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(None),
                "--plain" => o.plain = true,
                "--once" => o.samples = Some(1),
                "--pid" | "--samples" | "-t" | "-d" => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("{arg} requires a value"))?;
                    if arg == "-d" {
                        o.dump = Some(match value.as_str() {
                            "e" => 'e',
                            "a" => 'a',
                            _ => return Err("-d expects e or a".into()),
                        });
                    } else {
                        let n: u64 = value
                            .parse()
                            .map_err(|_| format!("invalid value for {arg}: {value}"))?;
                        if n == 0 {
                            return Err(format!("{arg} must be greater than zero"));
                        }
                        match arg.as_str() {
                            "-t" => o.interval = n,
                            "--pid" => o.pid = Some(n.try_into().map_err(|_| "PID exceeds u32")?),
                            _ => o.samples = Some(n),
                        }
                    }
                }
                _ if arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 1 => {
                    for flag in arg[1..].chars() {
                        match flag {
                            'c' => o.child = true,
                            'p' => o.parent = true,
                            'a' => o.anim = true,
                            'm' => o.memtree = true,
                            'g' => o.gspawn = true,
                            'r' => o.reset = true,
                            'f' => o.frametime = true,
                            's' => o.simple = true,
                            'v' => o.verbose = true,
                            _ => return Err(format!("unknown option -{flag}")),
                        }
                    }
                }
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }
        if !(o.child || o.parent || o.anim || o.memtree || o.gspawn || o.reset || o.frametime) {
            o.child = true;
            o.parent = true;
            o.anim = true;
            o.memtree = true;
            o.gspawn = true;
            o.reset = true;
            o.frametime = true;
        }
        Ok(Some(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(s: &[&str]) -> Result<Option<Options>, String> {
        Options::parse(s.iter().map(|s| s.to_string()))
    }
    #[test]
    fn flags_and_defaults() {
        let all = parse(&[]).unwrap().unwrap();
        assert!(all.reset && all.child && all.gspawn);
        let reset = parse(&["-rs", "-t", "10", "--samples", "2"])
            .unwrap()
            .unwrap();
        assert!(reset.reset && reset.simple && !reset.gspawn);
        assert_eq!(reset.interval, 10);
        assert_eq!(reset.samples, Some(2));
    }
    #[test]
    fn rejects_bad_arguments() {
        for args in [
            vec!["-t", "0"],
            vec!["-t", "-1"],
            vec!["-d", "x"],
            vec!["--pid"],
            vec!["--samples", "0"],
            vec!["-z"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
