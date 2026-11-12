use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "ath-world — serviço local de teste com estado persistente (SQLite)

uso:
  ath-world init  --db banco.sqlite --seed semente.json
      cria as contas de {\"accounts\": {\"ID\": \"saldo\"}} (apaga o estado anterior)
  ath-world serve --db banco.sqlite [--addr 127.0.0.1:0] [--port-file arquivo]
      atende por TCP; imprime o endereço e, com --port-file, grava-o no arquivo
  ath-world state --db banco.sqlite [--get caminho.do.campo]
      imprime o estado lendo o banco diretamente (não passa pelo serviço); com --get, só o
      valor do campo (ex.: counts.wires, accounts.ACME)
  ath-world log   --db banco.sqlite
      imprime o registro de chamadas do serviço
  ath-world fault --db banco.sqlite --tool T --kind refuse|lost|drop_reply|hang|status_unknown
                  [--nth N] [--ms M]
      injeta uma falha na N-ésima chamada a T (ou na próxima, sem --nth)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let run = || -> Result<(), String> {
        let cmd = args.first().ok_or(USAGE)?;
        let db = PathBuf::from(flag("--db").ok_or("informe --db")?);
        let e = |x: rusqlite::Error| x.to_string();
        match cmd.as_str() {
            "init" => {
                let seed = flag("--seed").ok_or("informe --seed")?;
                let text = std::fs::read_to_string(&seed).map_err(|x| format!("{seed}: {x}"))?;
                let j = serde_json::from_str(&text).map_err(|x| format!("{seed}: {x}"))?;
                let c = ath_world::open(&db).map_err(e)?;
                ath_world::seed(&c, &j)
            }
            "serve" => {
                let c = ath_world::open(&db).map_err(e)?;
                let l = TcpListener::bind(flag("--addr").unwrap_or_else(|| "127.0.0.1:0".into()))
                    .map_err(|x| x.to_string())?;
                let addr = l.local_addr().map_err(|x| x.to_string())?.to_string();
                println!("ath-world: {} em {addr}", db.display());
                let _ = std::io::stdout().flush();
                if let Some(pf) = flag("--port-file") {
                    // escreve e renomeia: quem espera pelo arquivo nunca o lê pela metade
                    let tmp = format!("{pf}.tmp");
                    std::fs::write(&tmp, &addr).and_then(|_| std::fs::rename(&tmp, &pf)).map_err(|x| x.to_string())?;
                }
                // com --sign-key, o serviço é testemunha de tempo e assina os recibos (v1.0-RC4)
                let key = match flag("--sign-key") {
                    Some(k) => Some(
                        arx::sign::parse_secret(&std::fs::read_to_string(&k).map_err(|x| format!("`{k}`: {x}"))?)
                            .map_err(|x| format!("`{k}`: {x}"))?,
                    ),
                    None => None,
                };
                ath_world::serve_with(l, c, key);
                Ok(())
            }
            "state" => {
                let c = ath_world::open(&db).map_err(e)?;
                let s = ath_world::state(&c).map_err(e)?;
                if let Some(path) = flag("--get") {
                    let v = path.split('.').fold(&s, |v, k| &v[k]);
                    match v {
                        serde_json::Value::String(t) => println!("{t}"),
                        other => println!("{other}"),
                    }
                    return Ok(());
                }
                println!("{}", serde_json::to_string_pretty(&s).map_err(|x| x.to_string())?);
                Ok(())
            }
            "log" => {
                let c = ath_world::open(&db).map_err(e)?;
                let mut st = c.prepare("SELECT line FROM log ORDER BY seq").map_err(e)?;
                for l in st.query_map([], |r| r.get::<_, String>(0)).map_err(e)? {
                    println!("{}", l.map_err(e)?);
                }
                Ok(())
            }
            "fault" => {
                let c = ath_world::open(&db).map_err(e)?;
                let nth = flag("--nth").map(|n| n.parse::<i64>().map_err(|_| "--nth inválido")).transpose()?;
                let ms = flag("--ms").and_then(|n| n.parse().ok()).unwrap_or(0);
                ath_world::add_fault(
                    &c,
                    &flag("--tool").ok_or("informe --tool")?,
                    nth,
                    &flag("--kind").ok_or("informe --kind")?,
                    ms,
                )
            }
            _ => Err(USAGE.into()),
        }
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            eprintln!("{m}");
            ExitCode::from(2)
        }
    }
}
