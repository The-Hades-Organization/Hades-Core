//! Corrida entre dois processos de verdade: o teste se reexecuta como filho (`HADES_CORRIDA_PAPEL`).

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use hades_core::atomic::write_file_atomic;
use hades_core::lock::{LockError, with_flock, with_flock_timeout};

const ITERACOES: u32 = 150;

fn filho(papel: &str, dir: &Path, extra: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "papel_filho", "--nocapture", "--test-threads=1"])
        .env("HADES_CORRIDA_PAPEL", papel)
        .env("HADES_CORRIDA_DIR", dir)
        .env("HADES_CORRIDA_EXTRA", extra)
        .stdout(Stdio::null())
        .spawn()
        .unwrap()
}

fn esperar_ok(filhos: Vec<std::process::Child>) {
    for f in filhos {
        let out = f.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "filho falhou: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

fn ler_contador(p: &Path) -> u32 {
    fs::read_to_string(p)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Roda só quando é filho; no processo normal passa sem fazer nada.
#[test]
fn papel_filho() {
    let Ok(papel) = std::env::var("HADES_CORRIDA_PAPEL") else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var("HADES_CORRIDA_DIR").unwrap());
    let extra = std::env::var("HADES_CORRIDA_EXTRA").unwrap_or_default();
    match papel.as_str() {
        "incrementa" => {
            for _ in 0..ITERACOES {
                with_flock(&dir.join("c.lock"), || {
                    let n = ler_contador(&dir.join("c.txt"));
                    std::thread::sleep(Duration::from_micros(200));
                    write_file_atomic(&dir.join("c.txt"), (n + 1).to_string().as_bytes()).unwrap();
                })
                .unwrap();
            }
        }
        "escreve" => {
            let conteudo = extra.repeat(4096);
            for _ in 0..ITERACOES {
                write_file_atomic(&dir.join("alvo.txt"), conteudo.as_bytes()).unwrap();
            }
        }
        "segura" => {
            with_flock(&dir.join("s.lock"), || {
                fs::write(dir.join("pronto"), "L").unwrap();
                std::thread::sleep(Duration::from_millis(1500));
            })
            .unwrap();
        }
        outro => panic!("papel desconhecido: {outro}"),
    }
}

#[test]
fn dois_processos_incrementando_sob_flock_nao_perdem_atualizacao() {
    let t = tempfile::tempdir().unwrap();
    esperar_ok(vec![
        filho("incrementa", t.path(), ""),
        filho("incrementa", t.path(), ""),
    ]);
    assert_eq!(ler_contador(&t.path().join("c.txt")), 2 * ITERACOES);
}

#[test]
fn dois_processos_escrevendo_atomico_nunca_deixam_arquivo_misto_nem_tmp() {
    let t = tempfile::tempdir().unwrap();
    let alvo = t.path().join("alvo.txt");
    let filhos = vec![
        filho("escreve", t.path(), "a"),
        filho("escreve", t.path(), "b"),
    ];
    let mut lidos = 0;
    while lidos < 2000 {
        if let Ok(s) = fs::read_to_string(&alvo) {
            assert_eq!(s.len(), 4096);
            assert!(
                s.chars().all(|c| c == 'a') || s.chars().all(|c| c == 'b'),
                "conteúdo misto"
            );
            lidos += 1;
        }
        if lidos > 0 && lidos % 500 == 0 {
            std::thread::yield_now();
        }
        if filhos.len() == 2 && lidos >= 2000 {
            break;
        }
    }
    esperar_ok(filhos);
    assert_eq!(hades_core::fsx::list_dir(t.path()), ["alvo.txt"]);
}

#[test]
fn lock_de_outro_processo_faz_with_flock_timeout_estourar_e_depois_entrar() {
    let t = tempfile::tempdir().unwrap();
    let mut f = filho("segura", t.path(), "");
    while !t.path().join("pronto").exists() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let lock = t.path().join("s.lock");
    let e = with_flock_timeout(&lock, Some(0.2), || ()).unwrap_err();
    assert!(matches!(e, LockError::Timeout(_)));
    assert_eq!(
        with_flock_timeout(&lock, Some(10.0), || "entrei").unwrap(),
        "entrei"
    );
    assert!(f.wait().unwrap().success());
}
