//! **v1.0-RC1, ponta a ponta.** O programa `examples/rc1/treasury.ath` executa num processo
//! `athc` de verdade, contra o serviço `ath-world` (SQLite em disco, TCP), e é interrompido com
//! `SIGKILL`. A recuperação roda em **outro processo** (`athc recover`), a partir do WAL.
//!
//! O que cada teste observa vem de fora do interpretador: o banco SQLite lido por outra
//! conexão, o WAL no disco e os códigos de saída dos processos. Nada aqui usa o `SimWorld`.

use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const REQUEST: &str = "\"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01.\"";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn fx(name: &str) -> PathBuf {
    root().join("examples/rc1/fixtures").join(name)
}

/// Um serviço `ath-world` próprio do teste: banco em disco, uma porta livre, uma thread.
struct World {
    dir: PathBuf,
    db: PathBuf,
    addr: String,
}

impl World {
    fn start(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ath-rc1-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("bank.sqlite");
        let c = ath_world::open(&db).unwrap();
        let seed: Json = serde_json::from_str(&std::fs::read_to_string(fx("seed.json")).unwrap()).unwrap();
        ath_world::seed(&c, &seed).unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || ath_world::serve(l, c));
        World { dir, db, addr }
    }

    /// O estado lido do banco por **outra** conexão: não passa pelo serviço nem pelo `athc`.
    fn state(&self) -> Json {
        ath_world::state(&ath_world::open(&self.db).unwrap()).unwrap()
    }

    fn count(&self, k: &str) -> u64 {
        self.state()["counts"][k].as_u64().unwrap()
    }

    fn log(&self) -> Vec<String> {
        let c = ath_world::open(&self.db).unwrap();
        let mut st = c.prepare("SELECT line FROM log ORDER BY seq").unwrap();
        let v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        v
    }

    fn fault(&self, tool: &str, kind: &str, ms: i64) {
        ath_world::add_fault(&ath_world::open(&self.db).unwrap(), tool, None, kind, ms).unwrap();
    }

    fn p(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn athc(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_athc"));
        c.env("NO_COLOR", "1");
        c
    }

    /// `athc grant --from intent`: a confirmação estruturada vira as concessões da execução.
    fn grant(&self, run: &str, intent: &str) -> PathBuf {
        let out = self.p(&format!("{run}.grants.json"));
        let o = self
            .athc()
            .args(["grant", "--run", run, "--origin", "confirmation:teste-rc1", "--out"])
            .arg(&out)
            .arg("--from")
            .arg(fx(intent))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        out
    }

    fn run_cmd(&self, run: &str, models: &str, grants: &Path) -> Command {
        let mut c = self.athc();
        c.arg("run")
            .arg(root().join("examples/rc1/treasury.ath"))
            .args(["--provider", "scripted", "--models"])
            .arg(fx(models))
            .args(["--host", &format!("tcp://{}", self.addr), "--trusted"])
            .arg(fx("trusted.json"))
            .args(["--run-id", run, "--timeout", "2", "--grants"])
            .arg(grants)
            .arg("--wal")
            .arg(self.p(&format!("{run}.wal")))
            .arg("--journal")
            .arg(self.p(&format!("{run}.journal.jsonl")))
            .arg("--report")
            .arg(self.p(&format!("{run}.report.json")))
            .args(["--arg", REQUEST]);
        c
    }

    fn recover_at(&self, run: &str, addr: &str, extra: &[&str]) -> Output {
        self.athc()
            .arg("recover")
            .arg(root().join("examples/rc1/treasury.ath"))
            .arg("--wal")
            .arg(self.p(&format!("{run}.wal")))
            .args(["--host", &format!("tcp://{addr}"), "--timeout", "2", "--report"])
            .arg(self.p(&format!("{run}.recover.json")))
            .args(extra)
            .output()
            .unwrap()
    }

    fn recover(&self, run: &str) -> Output {
        self.recover_at(run, &self.addr.clone(), &[])
    }

    fn wal(&self, run: &str) -> Vec<Json> {
        std::fs::read_to_string(self.p(&format!("{run}.wal")))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// Quantos lançamentos o razão tem para a chave de fio `run:kN` (a duplicação apareceria aqui).
    fn rows_for(&self, key: &str) -> usize {
        self.state()["ledger"].as_array().unwrap().iter().filter(|e| e["key"] == key).count()
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[cfg(unix)]
fn killed(o: &Output) -> bool {
    use std::os::unix::process::ExitStatusExt;
    o.status.signal() == Some(9)
}

/// Todo registro `intent` do WAL tem um desfecho posterior (`close` ou `escalate`).
fn every_intent_has_an_outcome(wal: &[Json]) -> bool {
    wal.iter().filter(|e| e["kind"] == "intent").all(|i| {
        wal.iter()
            .any(|e| (e["kind"] == "close" || e["kind"] == "escalate") && e["detail"]["key"] == i["detail"]["key"])
    })
}

/// **Uma operação legítima é executada e confirmada**, e o efeito é observável fora do
/// interpretador: saldos e razão no SQLite. Cada efeito cita no journal a concessão que o
/// autorizou; as concessões ficam consumidas no disco, e uma segunda execução com o mesmo
/// arquivo não as reutiliza.
#[test]
fn a_confirmed_intent_becomes_real_effects_once() {
    let w = World::start("legit");
    let before = w.state();
    assert_eq!(before["accounts"]["TREASURY"], "10000.00");
    let g = w.grant("legit", "intent.json");
    let o = w.run_cmd("legit", "models-legit.json", &g).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let s = w.state();
    assert_eq!(s["accounts"], json!({ "ACME": "120.00", "BETA": "75.50", "MALLORY": "0.00", "TREASURY": "9804.50" }));
    assert_eq!(s["counts"], json!({ "pays_posted": 2, "pays_refunded": 0, "receipts": 1, "scheduled": 1, "wires": 0 }));
    // o WAL: quatro intenções, quatro desfechos por resposta, a saga encerrada
    let wal = w.wal("legit");
    assert_eq!(wal[0]["kind"], "run");
    assert_eq!(wal.iter().filter(|e| e["kind"] == "intent").count(), 4);
    assert!(wal
        .iter()
        .filter(|e| e["kind"] == "close")
        .all(|e| e["detail"]["evidence"] == "reply" && e["detail"]["status"] == "done"));
    assert!(every_intent_has_an_outcome(&wal));
    // o journal: cada efeito autorizado por uma concessão desta execução, de origem confirmada
    let journal: Vec<Json> = std::fs::read_to_string(w.p("legit.journal.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let auth: Vec<&Json> = journal.iter().filter(|e| e["kind"] == "authorized").collect();
    assert_eq!(auth.len(), 3);
    for a in &auth {
        let by = a["detail"]["by"].as_array().unwrap().iter().find(|b| b.get("grant").is_some()).unwrap();
        assert_eq!((by["run"].as_str(), by["origin"].as_str()), (Some("legit"), Some("confirmation:teste-rc1")));
    }
    let spent = std::fs::read_to_string(format!("{}.spent", g.display())).unwrap();
    assert_eq!(spent.lines().count(), 3);
    // `athc recover` sobre uma execução que terminou: nada pendente, nada enviado
    let calls = w.log().len();
    let r = w.recover("legit");
    assert_eq!(r.status.code(), Some(0), "{}", text(&r));
    assert_eq!(w.log().len(), calls);
    // o mesmo WAL não aceita outra execução
    let again = w.run_cmd("legit", "models-legit.json", &g).output().unwrap();
    assert!(text(&again).contains("W0004"), "{}", text(&again));
    // outra execução (outro WAL) com o MESMO arquivo de concessões e a mesma identidade: as
    // concessões já foram consumidas, nada é pago de novo
    std::fs::rename(w.p("legit.wal"), w.p("legit.wal.done")).unwrap();
    let replay = w.run_cmd("legit", "models-legit.json", &g).output().unwrap();
    assert!(text(&replay).contains("nenhuma operação executada"), "{}", text(&replay));
    assert_eq!(w.state(), s);
}

/// **Uma operação proibida não produz seu efeito.** O extrato traz uma instrução injetada e o
/// modelo roteirizado a obedece, depois tenta as combinações que o produto cartesiano da Rev13
/// autorizaria (o valor de outra concessão, a operação de outra concessão). As três propostas
/// são recusadas antes de qualquer envio: o estado do serviço não muda e o WAL não tem intenção.
#[test]
fn injected_and_recombined_operations_never_reach_the_service() {
    let w = World::start("attack");
    let before = w.state();
    let g = w.grant("atk", "intent.json");
    let o = w.run_cmd("atk", "models-attack.json", &g).output().unwrap();
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(out.contains("rejeitada por SafePlan"), "{out}");
    assert!(out.contains("viola o contrato de `pay`") && out.contains("viola o contrato de `schedule`"), "{out}");
    assert_eq!(w.state(), before);
    assert!(w.log().iter().all(|l| l.starts_with("call statement")), "{:?}", w.log());
    assert_eq!(w.wal("atk").iter().filter(|e| e["kind"] == "intent").count(), 0);
    // nenhuma concessão foi consumida
    assert!(!PathBuf::from(format!("{}.spent", g.display())).exists());
    // sem concessão alguma, o plano legítimo também não executa: falha fechado
    let empty = w.p("none.grants.json");
    std::fs::write(&empty, "{\"grants\": []}").unwrap();
    let o = w.run_cmd("none", "models-legit.json", &empty).output().unwrap();
    assert!(text(&o).contains("nenhuma operação executada"), "{}", text(&o));
    assert_eq!(w.state(), before);
}

/// **Interrupção real e recuperação, em todos os pontos.** O processo `athc run` recebe
/// `SIGKILL` em cada ponto do protocolo (intenção persistida, enviado, desfecho conhecido,
/// desfecho persistido) de cada uma das quatro operações. Um processo novo (`athc recover`)
/// retoma a execução a partir do WAL. Em todos os 16 casos:
///
/// * o processo morreu por sinal 9 (não é pânico capturado);
/// * a recuperação termina sem pendência e toda intenção tem desfecho com evidência;
/// * **nenhuma chave tem mais de um lançamento** no razão: o reenvio idempotente não duplica;
/// * a queda dentro da saga (operações 1 e 2) deixa o lote estornado e os saldos de volta;
/// * depois da saga, os pagamentos ficam; o agendamento (`queryable`) é resolvido por consulta
///   e nunca reenviado; o comprovante (`idempotent`) existe exatamente uma vez;
/// * recuperar de novo não envia nada.
#[cfg(unix)]
#[test]
fn sigkill_at_every_point_is_recovered_by_a_new_process() {
    let w = World::start("kill");
    let mut expect_pays = 0;
    let mut expect_refunds = 0;
    let mut expect_sched = 0;
    let mut expect_receipts = 0;
    for nth in 1..=4u32 {
        for point in ["intent", "sent", "confirmed", "closed"] {
            let run = format!("k-{point}-{nth}");
            let g = w.grant(&run, "intent.json");
            let o = w
                .run_cmd(&run, "models-legit.json", &g)
                .args(["--kill-at", &format!("{point}:{nth}")])
                .output()
                .unwrap();
            assert!(killed(&o), "{run}: o processo não morreu por SIGKILL\n{}", text(&o));
            // o que a queda deixou em aberto está no WAL
            let wal = w.wal(&run);
            let open = !every_intent_has_an_outcome(&wal);
            assert_eq!(open, point != "closed", "{run}: {wal:?}");
            let r = w.recover(&run);
            assert_eq!(r.status.code(), Some(0), "{run}\n{}", text(&r));
            let wal = w.wal(&run);
            assert!(every_intent_has_an_outcome(&wal), "{run}");
            assert!(
                wal.iter().filter(|e| e["kind"] == "saga_begin").count()
                    == wal.iter().filter(|e| e["kind"] == "saga_end").count()
            );
            for k in 1..=4 {
                assert!(w.rows_for(&format!("{run}:k{k}")) <= 1, "{run}: a chave k{k} tem mais de um lançamento");
            }
            // o que o mundo tem de ter depois desta execução
            if nth <= 2 {
                // a saga foi interrompida: os `nth` pagamentos que saíram ou podem ter saído são
                // estornados. Se a queda foi antes do envio, a recuperação completa a intenção
                // persistida (reenvio idempotente) e estorna em seguida: o lote é tudo-ou-nada.
                expect_refunds += nth as u64;
            } else {
                expect_pays += 2;
                // queryable: existe se saiu do processo; a recuperação só consulta, não reenvia
                if nth > 3 || point != "intent" {
                    expect_sched += 1;
                }
                // idempotent fora de saga: a recuperação completa a intenção persistida
                if nth == 4 {
                    expect_receipts += 1;
                }
            }
            let s = w.state();
            assert_eq!(s["counts"]["pays_posted"], expect_pays, "{run}: {}", s["counts"]);
            assert_eq!(s["counts"]["pays_refunded"], expect_refunds, "{run}: {}", s["counts"]);
            assert_eq!(s["counts"]["scheduled"], expect_sched, "{run}: {}", s["counts"]);
            assert_eq!(s["counts"]["receipts"], expect_receipts, "{run}: {}", s["counts"]);
            // recuperar de novo: nada a fazer, nada enviado
            let calls = w.log().len();
            let r2 = w.recover(&run);
            assert_eq!(r2.status.code(), Some(0), "{run}\n{}", text(&r2));
            assert_eq!(w.log().len(), calls, "{run}: a segunda recuperação enviou algo");
        }
    }
    // a partida dobrada fecha: o dinheiro que saiu da tesouraria está nas contas dos pagos
    let s = w.state();
    let paid = 195.5 * (expect_pays as f64 / 2.0);
    assert_eq!(s["accounts"]["TREASURY"], format!("{:.2}", 10000.0 - paid));
    assert!(w.log().iter().any(|l| l.contains("recibo repetido")), "nenhum reenvio idempotente foi exercitado");
}

/// **Um provedor opaco com resultado desconhecido é escalado.** A remessa (`wire`, `opaque`)
/// saiu e o processo morreu antes da resposta. O serviço aplicou o efeito, mas não oferece
/// chave nem consulta: a recuperação **não** inventa "feito" nem "não feito", escala e sai com
/// código 3. O mesmo vale quando a queda foi antes do envio: do WAL, os dois casos são
/// indistinguíveis, e os dois ficam escalados (`opaque_not_exactly_once`, em `Wal.lean`).
#[cfg(unix)]
#[test]
fn an_opaque_unknown_outcome_is_escalated_never_guessed() {
    let w = World::start("opaque");
    // `wires_after` é o total no serviço: a primeira execução remete (morre depois do envio), a
    // segunda morre antes de enviar e não acrescenta nada
    for (point, wires_after) in [("sent", 1u64), ("intent", 1u64)] {
        let run = format!("wire-{point}");
        let g = w.grant(&run, "intent-wire.json");
        let o = w.run_cmd(&run, "models-wire.json", &g).args(["--kill-at", &format!("{point}:2")]).output().unwrap();
        assert!(killed(&o), "{}", text(&o));
        // o efeito da primeira remessa aconteceu de verdade; o da segunda, não
        assert_eq!(w.count("wires"), wires_after);
        let r = w.recover(&run);
        let out = text(&r);
        assert_eq!(r.status.code(), Some(3), "{out}");
        assert!(out.contains("ESCALADA") && out.contains("opaco"), "{out}");
        let wal = w.wal(&run);
        let esc: Vec<&Json> = wal.iter().filter(|e| e["kind"] == "escalate").collect();
        assert_eq!(esc.len(), 1);
        assert_eq!(esc[0]["detail"]["key"], "k2");
        assert!(!wal.iter().any(|e| e["kind"] == "close" && e["detail"]["key"] == "k2"));
        // nenhuma remessa foi reenviada pela recuperação
        assert_eq!(w.count("wires"), wires_after);
        // continua exigindo intervenção, inclusive com --retry-escalated
        assert_eq!(w.recover(&run).status.code(), Some(3));
        assert_eq!(w.recover_at(&run, &w.addr.clone(), &["--retry-escalated"]).status.code(), Some(3));
        assert_eq!(w.count("wires"), wires_after);
        let rep: Json =
            serde_json::from_str(&std::fs::read_to_string(w.p(&format!("{run}.recover.json"))).unwrap()).unwrap();
        assert_eq!(rep["needs_intervention"], true);
        assert_eq!(rep["after"]["escalated"], json!(["k2"]));
    }
}

/// **Morte por um sinal externo**, não por um ponto escolhido pelo próprio processo: o serviço
/// aplica o comprovante, confirma no disco e segura a resposta; o teste espera o efeito aparecer
/// no banco e mata o `athc` com `SIGKILL`. O processo novo reenvia com a mesma chave e recebe o
/// recibo: um comprovante, não dois.
#[cfg(unix)]
#[test]
fn an_external_sigkill_while_waiting_for_the_reply_is_recovered() {
    let w = World::start("extkill");
    w.fault("receipt", "hang", 1500);
    let g = w.grant("ext", "intent.json");
    let mut child =
        w.run_cmd("ext", "models-legit.json", &g).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let t0 = std::time::Instant::now();
    while w.count("receipts") == 0 {
        assert!(t0.elapsed().as_secs() < 20, "o comprovante não chegou ao serviço");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.kill().unwrap(); // SIGKILL
    let status = child.wait().unwrap();
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9));
    }
    assert!(!every_intent_has_an_outcome(&w.wal("ext")));
    let r = w.recover("ext");
    assert_eq!(r.status.code(), Some(0), "{}", text(&r));
    assert!(text(&r).contains("recovery:resend"), "{}", text(&r));
    assert_eq!(w.count("receipts"), 1);
    assert_eq!(w.rows_for("ext:k4"), 1);
    assert!(w.log().iter().any(|l| l.contains("receipt ext:k4: recibo repetido")), "{:?}", w.log());
}

/// **Uma falha de leitura do WAL não vira ausência de obrigações.** Com uma obrigação em aberto
/// no WAL de uma execução morta: o arquivo ilegível (`W0001`), corrompido no meio (`W0002`),
/// ausente (`W0003`) ou de outro programa (`W0004`) faz `athc recover` sair com código 2, sem
/// enviar nada e sem escrever no WAL. Uma última linha interrompida é o único defeito tolerado:
/// é reportada, guardada em `.torn`, e a recuperação segue com os registros íntegros.
#[cfg(unix)]
#[test]
fn a_wal_that_cannot_be_read_stops_recovery_instead_of_emptying_it() {
    let w = World::start("walfail");
    let g = w.grant("wf", "intent.json");
    let o = w.run_cmd("wf", "models-legit.json", &g).args(["--kill-at", "sent:3"]).output().unwrap();
    assert!(killed(&o));
    let good = std::fs::read(w.p("wf.wal")).unwrap();
    let calls = w.log().len();
    let refuse = |code: &str| {
        let r = w.recover("wf");
        let out = text(&r);
        assert_eq!(r.status.code(), Some(2), "{code}: {out}");
        assert!(out.contains(code), "{code}: {out}");
        assert_eq!(w.log().len(), calls, "{code}: a recuperação enviou algo");
    };
    // corrupção no meio: um byte trocado numa linha inteira
    let mut bad = good.clone();
    let i = bad.iter().position(|&b| b == b'\n').unwrap() + 40;
    bad[i] ^= 0x01;
    std::fs::write(w.p("wf.wal"), &bad).unwrap();
    refuse("W0002");
    assert_eq!(std::fs::read(w.p("wf.wal")).unwrap(), bad, "o WAL corrompido foi alterado");
    // uma linha do meio removida
    let text_wal = String::from_utf8(good.clone()).unwrap();
    let lines: Vec<&str> = text_wal.lines().collect();
    let gap: String = lines.iter().enumerate().filter(|(i, _)| *i != 2).map(|(_, l)| format!("{l}\n")).collect();
    std::fs::write(w.p("wf.wal"), gap).unwrap();
    refuse("W0002");
    // ilegível: um diretório no lugar do arquivo (falha de leitura para qualquer usuário)
    std::fs::remove_file(w.p("wf.wal")).unwrap();
    std::fs::create_dir(w.p("wf.wal")).unwrap();
    refuse("W0001");
    std::fs::remove_dir(w.p("wf.wal")).unwrap();
    // ausente: não é "nada a recuperar"
    refuse("W0003");
    // o registro da execução adulterado: quebra o hash antes de qualquer outra conferência
    let other = text_wal.replacen("\"program\":\"", "\"program\":\"00", 1);
    std::fs::write(w.p("wf.wal"), other).unwrap();
    refuse("W0002");
    // íntegro, mas de outro programa
    std::fs::write(w.p("wf.wal"), &good).unwrap();
    let r = w
        .athc()
        .arg("recover")
        .arg(root().join("examples/remediate.ath"))
        .arg("--wal")
        .arg(w.p("wf.wal"))
        .args(["--host", &format!("tcp://{}", w.addr)])
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(2), "{}", text(&r));
    assert!(text(&r).contains("W0004"), "{}", text(&r));
    assert_eq!(w.log().len(), calls);
    // a obrigação continua lá: com o WAL íntegro e uma cauda rasgada, a recuperação a resolve
    let mut torn = good.clone();
    torn.extend_from_slice(b"{\"detail\":{\"key\":\"k9\",\"tool\":\"pa");
    std::fs::write(w.p("wf.wal"), torn).unwrap();
    let r = w.recover("wf");
    let out = text(&r);
    assert_eq!(r.status.code(), Some(0), "{out}");
    assert!(out.contains("última linha interrompida"), "{out}");
    assert!(w.p("wf.wal.torn").exists());
    assert!(out.contains("recovery:status"), "{out}");
    assert_eq!(w.count("scheduled"), 1);
    assert!(every_intent_has_an_outcome(&w.wal("wf")));
}

/// O provedor fora do ar durante a recuperação não é evidência de nada: as intenções continuam
/// em aberto (não viram "não aplicada"), a saída é 3, e uma nova recuperação com o serviço no
/// ar as resolve.
#[cfg(unix)]
#[test]
fn an_unreachable_provider_leaves_obligations_open_not_failed() {
    let w = World::start("down");
    let g = w.grant("down", "intent.json");
    let o = w.run_cmd("down", "models-legit.json", &g).args(["--kill-at", "sent:1"]).output().unwrap();
    assert!(killed(&o));
    // uma porta sem ninguém ouvindo
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    };
    let r = w.recover_at("down", &dead, &[]);
    assert_eq!(r.status.code(), Some(3), "{}", text(&r));
    let wal = w.wal("down");
    assert!(!wal.iter().any(|e| e["kind"] == "close"), "o provedor inalcançável virou um desfecho: {wal:?}");
    assert!(!wal.iter().any(|e| e["kind"] == "saga_end"));
    assert_eq!(w.count("pays_refunded"), 0);
    // com o serviço no ar: a escalada sem resposta é retomada, o pagamento é confirmado por
    // reenvio (sem duplicar) e a saga interrompida é estornada
    let r = w.recover_at("down", &w.addr.clone(), &["--retry-escalated"]);
    assert_eq!(r.status.code(), Some(0), "{}", text(&r));
    assert_eq!((w.count("pays_posted"), w.count("pays_refunded")), (0, 1));
    assert_eq!(w.rows_for("down:k1"), 1);
}

/// Sem matar o processo: o serviço aplica e descarta a resposta (`drop_reply`). O `athc`
/// reconcilia **dentro** da execução pelo protocolo da classe — reenvio com a mesma chave para o
/// pagamento (`idempotent`), consulta para o agendamento (`queryable`) — e termina com os efeitos
/// certos, uma vez cada. Para a remessa (`opaque`) não há protocolo: o programa transfere a
/// obrigação, o WAL a registra como escalada e `athc recover --inspect` a aponta.
#[test]
fn lost_replies_are_reconciled_in_process_by_provider_class() {
    let w = World::start("drop");
    w.fault("pay", "drop_reply", 0);
    w.fault("schedule", "drop_reply", 0);
    let g = w.grant("drop", "intent.json");
    let o = w.run_cmd("drop", "models-legit.json", &g).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let s = w.state();
    assert_eq!(s["counts"], json!({ "pays_posted": 2, "pays_refunded": 0, "receipts": 1, "scheduled": 1, "wires": 0 }));
    assert_eq!((w.rows_for("drop:k1"), w.rows_for("drop:k3")), (1, 1));
    assert!(every_intent_has_an_outcome(&w.wal("drop")));

    w.fault("wire", "drop_reply", 0);
    let g = w.grant("dropw", "intent-wire.json");
    let o = w.run_cmd("dropw", "models-wire.json", &g).output().unwrap();
    let out = text(&o);
    assert!(out.contains("pendente"), "{out}");
    assert_eq!(w.count("wires"), 1);
    let insp = w
        .athc()
        .arg("recover")
        .arg(root().join("examples/rc1/treasury.ath"))
        .arg("--wal")
        .arg(w.p("dropw.wal"))
        .arg("--inspect")
        .output()
        .unwrap();
    assert_eq!(insp.status.code(), Some(3), "{}", text(&insp));
    assert!(text(&insp).contains("opaco"), "{}", text(&insp));
}

/// A confirmação estruturada: uma intenção ambígua não vira concessão, e uma origem que não é
/// uma confirmação do principal (o pedido em texto, um arquivo, o modelo) não confere autoridade.
#[test]
fn an_ambiguous_intent_or_an_unconfirmed_origin_grants_nothing() {
    let w = World::start("grantcli");
    let out = w.p("amb.grants.json");
    let o = w
        .athc()
        .args(["grant", "--run", "r", "--origin", "confirmation:t", "--out"])
        .arg(&out)
        .arg("--from")
        .arg(fx("intent-ambiguous.json"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("ambígua") && !out.exists(), "{}", text(&o));
    for origin in ["request:texto", "file:bill.txt", "model:planner", "extracted"] {
        let o = w
            .athc()
            .args(["grant", "--run", "r", "--origin", origin, "--op", "pay", "--arg", "ACME", "--arg", "1.00", "--out"])
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{origin}: {}", text(&o));
        assert!(!out.exists(), "{origin}");
    }
    // a tupla exata, confirmada: concede; duas confirmações iguais são duas concessões
    for _ in 0..2 {
        let o = w
            .athc()
            .args([
                "grant",
                "--run",
                "r",
                "--origin",
                "confirmation:t",
                "--op",
                "pay",
                "--arg",
                "ACME",
                "--arg",
                "1.00",
                "--out",
            ])
            .arg(&out)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", text(&o));
    }
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(j["grants"].as_array().unwrap().len(), 2);
    assert_ne!(j["grants"][0]["id"], j["grants"][1]["id"]);
}
