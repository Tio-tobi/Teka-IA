//! Quantas ferramentas do Harness servem pela ponte? A medição de verdade.
//!
//! Duas tentativas anteriores erraram, e em direções opostas:
//!
//! - **Argumento vazio.** Sub-conta: `read_image` reclama do `file_path` que falta
//!   ANTES de chegar na checagem de agente, e parece livre.
//! - **Grep na fonte por `exec.agent`.** Super-conta: `fs-search` menciona quatro
//!   vezes e o `grep` dele funciona — `exec.agent?.` opcional não é exigência.
//!
//! Só chamar com argumento VÁLIDO responde. É o que isto faz.
//!
//! ## O que não é chamado, e por quê
//!
//! `ralph`, `subagent`, `subagent_fork`, `workflow` e `skill` disparam laço de
//! agente. Se funcionassem, custariam tempo e dinheiro de verdade. Ficam de fora e
//! contam como NÃO MEDIDO — que é diferente de "não serve".
use std::collections::BTreeMap;
use teka::json::{self, Json};
use teka::tools::harness_tcp;

/// As que disparam agente. Não são chamadas.
const PERIGOSAS: &[&str] = &["ralph", "subagent", "subagent_fork", "workflow", "skill"];

/// Um valor plausível para uma propriedade, pelo nome e pelo tipo.
fn valor(prop: &str, tipo: &str, area: &str) -> Json {
    let p = prop.to_lowercase();
    match tipo {
        "number" | "integer" => Json::Num(1.0),
        "boolean" => Json::Bool(false),
        "array" => Json::Lista(vec![]),
        _ => {
            if p.contains("image") || prop == "file_path" {
                json::txt(&format!("{area}/alvo.png"))
            } else if p.contains("path") || p.contains("file") || p.contains("dir") {
                json::txt(&format!("{area}/alvo.txt"))
            } else if p.contains("pattern") || p.contains("query") || p.contains("search") {
                json::txt("teka")
            } else if p.contains("command") || p.contains("cmd") || p.contains("script") {
                json::txt("echo teka")
            } else if p.contains("content") || p.contains("text") || p.contains("new_str") {
                json::txt("teka")
            } else {
                json::txt("teka")
            }
        }
    }
}

fn main() {
    let area = std::env::temp_dir().join("teka_estado");
    let _ = std::fs::remove_dir_all(&area);
    std::fs::create_dir_all(&area).expect("area");
    std::fs::write(area.join("alvo.txt"), b"teka aqui").expect("alvo");
    // PNG 1x1 DE VERDADE. Sem ele, `read_image` morre na checagem de extensao antes
    // da checagem de agente e parece que serve -- foi assim que as duas medicoes
    // anteriores o classificaram errado.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
        0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
        0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(area.join("alvo.png"), PNG).expect("png");
    let area_s = area.to_string_lossy().replace('\\', "/");

    let endereco = std::env::var("TEKA_PONTE_ENDERECO")
        .unwrap_or_else(|_| harness_tcp::ENDERECO_PADRAO.to_string());
    let segredo = match teka::tools::ponte_auto::garantir(&endereco) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let prazo = std::time::Duration::from_secs(60);

    // A lista CRUA, com os esquemas — `harness_tcp::listar` joga fora os parâmetros.
    let mut c = match harness_tcp::conectar(&endereco, &segredo, prazo) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let r = match c.pedir("tools/list", json::obj(vec![])) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let lista = r.get("tools").and_then(Json::lista).map(<[Json]>::to_vec).unwrap_or_default();
    println!("\n  {} ferramentas\n", lista.len());

    let mut grupos: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for t in &lista {
        let nome = t.get("name").and_then(Json::texto).unwrap_or("").to_string();
        if nome.is_empty() {
            continue;
        }
        if PERIGOSAS.contains(&nome.as_str()) {
            grupos.entry("NAO MEDIDO (dispara agente)").or_default().push(nome);
            continue;
        }
        // Monta os argumentos obrigatórios a partir do esquema.
        let mut args = Vec::new();
        if let Some(req) = t.get("parameters.required").and_then(Json::lista) {
            for nome_prop in req.iter().filter_map(Json::texto) {
                let tipo = t
                    .get(&format!("parameters.properties.{nome_prop}.type"))
                    .and_then(Json::texto)
                    .unwrap_or("string")
                    .to_string();
                args.push((nome_prop.to_string(), valor(nome_prop, &tipo, &area_s)));
            }
        }
        let mut c2 = match harness_tcp::conectar(&endereco, &segredo, prazo) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let resp = harness_tcp::chamar(&mut c2, &nome, Json::Obj(args));
        let texto = match &resp {
            Ok(r) => r.texto.clone(),
            Err(e) => e.clone(),
        };
        let baixo = texto.to_lowercase();
        let g = if matches!(&resp, Ok(r) if !r.erro) {
            "SERVE (executou)"
        } else if baixo.contains("calling agent")
            || baixo.contains("model route")
            || baixo.contains("not live in this store")
            || baixo.contains("exec.agent")
            // "owning agent session" do `todo_write` -- minha primeira lista de
            // palavras nao pegava, e ele aparecia como se servisse.
            || baixo.contains("owning agent")
            || baixo.contains("agent session")
        {
            "NAO SERVE (exige agente/sessao)"
        } else {
            "SERVE (chegou na logica, errou por outro motivo)"
        };
        grupos
            .entry(g)
            .or_default()
            .push(format!("{nome:<22} {}", texto.chars().take(64).collect::<String>()));
    }

    let total: usize = grupos.values().map(|v| v.len()).sum();
    for (g, v) in &grupos {
        println!("  === {g}: {} de {total}", v.len());
        for l in v {
            println!("      {l}");
        }
        println!();
    }
    let _ = std::fs::remove_dir_all(&area);
}
