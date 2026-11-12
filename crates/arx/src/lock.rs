//! Exclusão mútua entre **processos** por `flock(2)` (v1.0-RC2).
//!
//! Duas travas, as duas consultivas e liberadas pelo núcleo quando o processo morre (inclusive
//! por `SIGKILL`), de modo que uma queda nunca deixa trava órfã:
//!
//! * a do **WAL** (`<wal>.lock`), não bloqueante e mantida pela vida do processo: um executor
//!   por WAL. O segundo `athc run`/`athc recover` no mesmo WAL para com `W0006`;
//! * a do **registro de consumo** (`<concessões>.spent`), bloqueante e curta: a seção crítica
//!   que relê o registro, confere que a concessão está livre, persiste a intenção e o consumo.

use std::fs::File;

#[cfg(unix)]
pub fn exclusive(f: &File, block: bool) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    extern "C" {
        fn flock(fd: i32, op: i32) -> i32;
    }
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    loop {
        // SAFETY: `flock` só recebe um descritor válido, que `f` mantém aberto.
        let r = unsafe { flock(f.as_raw_fd(), if block { LOCK_EX } else { LOCK_EX | LOCK_NB }) };
        if r == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Sem `flock`, a exclusão não pode ser garantida: falha fechado.
#[cfg(not(unix))]
pub fn exclusive(_f: &File, _block: bool) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "exclusão entre processos exige flock(2)"))
}
