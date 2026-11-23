//! Casos de `loop/test/ts/limites.test.ts` (amostras, retenção e medida atual) e da borda de leitura com HOME falso.

use std::fs;

use serde_json::json;

use super::*;
use crate::journal::iso_ms;

const T0: i64 = 1_793_527_200; // 2026-11-01T10:00:00Z
const RENOVA: f64 = (T0 + 5 * 3600) as f64;

fn at(s: i64) -> String {
    iso_ms(s * 1000)
}

fn sample(s: i64, five: f64, resets_at: f64) -> Amostra {
    Amostra {
        at: at(s),
        five: Some(Leitura {
            pct: five,
            resets_at,
        }),
        seven: None,
        version: None,
    }
}

fn s5(s: i64, five: f64) -> Amostra {
    sample(s, five, RENOVA)
}

mod amostras {
    use super::*;

    #[test]
    fn dado_arquivo_com_linha_corrompida_entao_so_ela_e_ignorada_e_as_demais_vem_em_ordem_de_horario()
     {
        let grava = |a: &Amostra| {
            json!({"at": a.at, "five": {"pct": a.five.unwrap().pct, "resetsAt": a.five.unwrap().resets_at}})
                .to_string()
        };
        let linhas = [
            grava(&s5(T0 + 60, 5.0)),
            "lixo".to_owned(),
            json!({"at": "x"}).to_string(),
            grava(&s5(T0, 4.0)),
            String::new(),
        ]
        .join("\n");
        let pcts: Vec<f64> = amostras_de(&linhas)
            .iter()
            .map(|a| a.five.unwrap().pct)
            .collect();
        assert_eq!(pcts, [4.0, 5.0]);
    }

    #[test]
    fn dado_linha_gravada_sem_janela_valida_ou_horario_invalido_entao_descarta() {
        let linhas = [
            json!({"at": "2026-11-01T10:00:00.000Z"}).to_string(),
            json!({"at": "2026-11-01T10:00:00.000Z", "five": {"pct": -1, "resetsAt": 5}}).to_string(),
            json!({"at": 5, "five": {"pct": 1, "resetsAt": 5}}).to_string(),
            json!({"at": "2026-11-01T10:00:00.000Z", "five": {"pct": "9", "resetsAt": 5}}).to_string(),
            json!({"at": "2026-11-01T10:00:00.000Z", "seven": {"pct": 1, "resetsAt": 5}, "version": "9"})
                .to_string(),
        ]
        .join("\n");
        let lidas = amostras_de(&linhas);
        assert_eq!(lidas.len(), 1);
        assert_eq!(lidas[0].version.as_deref(), Some("9"));
        assert_eq!(lidas[0].five, None);
        assert_eq!(
            lidas[0].janela(Janela::Seven),
            Some(Leitura {
                pct: 1.0,
                resets_at: 5.0
            })
        );
    }

    #[test]
    fn dado_horario_com_fuso_entao_a_ordem_segue_o_instante() {
        let linhas = [
            json!({"at": "2026-11-01T10:00:00-03:00", "five": {"pct": 2, "resetsAt": 5}})
                .to_string(),
            json!({"at": "2026-11-01T12:00:00Z", "five": {"pct": 1, "resetsAt": 5}}).to_string(),
        ]
        .join("\n");
        let pcts: Vec<f64> = amostras_de(&linhas)
            .iter()
            .map(|a| a.five.unwrap().pct)
            .collect();
        assert_eq!(pcts, [1.0, 2.0]);
    }
}

mod retencao {
    use super::*;

    #[test]
    fn dado_retencao_entao_descarta_o_mais_velho_que_o_corte() {
        let amostras = [s5(T0 - 3 * 86_400, 1.0), s5(T0 - 86_400, 2.0)];
        let now = (T0 * 1000) as f64;
        assert_eq!(dentro_da_retencao(&amostras, now, 2.0).len(), 1);
        assert_eq!(dentro_da_retencao(&amostras, now, 3.0).len(), 2);
    }
}

mod medida_atual {
    use super::*;

    #[test]
    fn dado_janela_ja_renovada_entao_nao_conta_como_medida_a_ainda_aberta_sim() {
        let amostras = [Amostra {
            at: at(T0),
            five: Some(Leitura {
                pct: 90.0,
                resets_at: (T0 - 1) as f64,
            }),
            seven: Some(Leitura {
                pct: 40.0,
                resets_at: (T0 + 86_400) as f64,
            }),
            version: None,
        }];
        let a = atuais(&amostras, (T0 * 1000) as f64);
        assert_eq!(a.five, None);
        assert_eq!(a.get(Janela::Seven).unwrap().pct, 40.0);
    }

    #[test]
    fn dado_duas_amostras_entao_vale_a_ultima() {
        let a = atuais(
            &[s5(T0, 10.0), s5(T0 + 60, 12.0)],
            ((T0 + 120) * 1000) as f64,
        );
        assert_eq!(a.five.unwrap().pct, 12.0);
    }

    #[test]
    fn dado_renovacao_igual_ao_agora_entao_a_janela_ja_venceu() {
        let a = atuais(&[sample(T0, 5.0, T0 as f64)], (T0 * 1000) as f64);
        assert_eq!(a.five, None);
    }

    #[test]
    fn dado_atual_entao_o_json_segue_a_ordem_do_ts() {
        let a = atuais(&[s5(T0, 10.5)], (T0 * 1000) as f64);
        assert_eq!(
            a.five.unwrap().to_value().to_string(),
            format!(r#"{{"pct":10.5,"resetsAt":{RENOVA:.1},"at":"2026-11-01T10:00:00.000Z"}}"#)
        );
    }

    #[test]
    fn dado_janela_entao_chave_e_rotulo_seguem_o_ts() {
        let chaves: Vec<&str> = JANELAS.iter().map(|j| j.key()).collect();
        let rotulos: Vec<&str> = JANELAS.iter().map(|j| j.rotulo()).collect();
        assert_eq!(chaves, ["five", "seven"]);
        assert_eq!(rotulos, ["5 h", "semana"]);
    }
}

mod borda {
    use super::*;

    fn home() -> (tempfile::TempDir, Env) {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::empty().with("HOME", dir.path().to_str().unwrap());
        (dir, env)
    }

    #[test]
    fn dado_nenhuma_variavel_entao_a_retencao_e_30_dias() {
        let (dir, env) = home();
        assert_eq!(retention_days(&env, dir.path()), 30.0);
    }

    #[test]
    fn dado_retencao_valida_entao_vale_o_valor() {
        let (dir, env) = home();
        for (raw, esperado) in [("1", 1.0), ("365", 365.0), (" 7 ", 7.0), ("0x10", 16.0)] {
            let env = env.clone().with("HADES_LIMITS_RETENTION_DAYS", raw);
            assert_eq!(retention_days(&env, dir.path()), esperado, "{raw}");
        }
    }

    #[test]
    fn dado_retencao_fora_da_faixa_entao_so_avisa_e_usa_30() {
        let (dir, env) = home();
        for raw in ["abc", "0", "366", "1.5", "-3", "Infinity"] {
            let env = env.clone().with("HADES_LIMITS_RETENTION_DAYS", raw);
            assert_eq!(retention_days(&env, dir.path()), 30.0, "{raw}");
        }
    }

    #[test]
    fn dado_sem_arquivo_entao_nenhuma_amostra_e_nenhuma_pasta_criada() {
        let (dir, env) = home();
        assert!(ler_amostras(&env).is_empty());
        assert!(!dir.path().join(".claude").exists());
    }

    #[test]
    fn dado_arquivo_de_amostras_entao_le_retem_e_mede_a_janela_aberta() {
        let (dir, env) = home();
        let file = dir.path().join(".claude/hades/limits/samples.jsonl");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let fecha = T0 + 3 * 3600;
        let linha = |s: i64, five: f64| {
            json!({"at": at(s), "five": {"pct": five, "resetsAt": fecha}}).to_string()
        };
        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                linha(T0 - 40 * 86_400, 20.0),
                linha(T0 - 60, 23.0)
            ),
        )
        .unwrap();
        let now = (T0 * 1000) as f64;
        let amostras =
            dentro_da_retencao(&ler_amostras(&env), now, retention_days(&env, dir.path()));
        assert_eq!(amostras.len(), 1);
        let a = atuais(&amostras, now);
        assert_eq!(a.five.unwrap().pct, 23.0);
        assert_eq!(a.seven, None);
    }
}
