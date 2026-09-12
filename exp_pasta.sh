#!/usr/bin/env bash
# ===================================================================
# DESTINO DE PASTA — REGISTRADO EM 2026-09-12, ANTES DE RODAR
# ===================================================================
#
# `copiar_arquivo` e `mover_arquivo` deram ZERO no benchmark de 329 (36 e 60
# tentativas). Causa medida: nenhum molde ensinava destino de PASTA, e a primitiva
# recusava pasta como destino. Consertado em `a5ef7db`, nas duas pontas.
#
#   PRIMARIO   `ferramenta certa` de 329, pareado por semente, contra `b329_*`.
#              Base: 216,58 (65,8%), medida hoje no HEAD anterior.
#
#   SECUNDARIO acerto de `copiar_arquivo` e `mover_arquivo`. Base: ZERO nas duas.
#
#   n = 12, FIXADO. `analisa_pasta.py` recusa parcial.
#
# -------------------------------------------------------------------
# A EXPECTATIVA, E O QUE A FALSEIA
# -------------------------------------------------------------------
#
# As duas saem do zero. Se nao sairem, o mecanismo que escrevi esta errado -- e o
# mecanismo aqui NAO e palpite: esta medido que a palavra "pasta" so existia com as
# ferramentas de pasta, e que as duas eram as unicas sem a forma.
#
# O PRIMARIO e que e incerto, e por dois motivos opostos:
#
#   a favor   as duas saem do zero e valem 8 frases das 329
#   contra    16 moldes novos e o destino misto (45% PASTAS) mexem no espaco de
#             TODAS as ferramentas de caminho. `listar_pasta` e `criar_pasta`
#             podem PERDER, porque "pasta" deixou de ser sinal exclusivo delas
#
# Esse "contra" e a suspeita nomeada de antemao, e e onde eu olharia primeiro se o
# primario nao mover. Registro sabendo que meu placar de prever superficie e de
# cinco tentativas e zero acertos -- por isso o primario e a medida AMPLA.
#
# CHAO: o desvio entre sementes na regua de 329 mediu 3,64 pp (11,97 de 329). Com
# n=12 isso detecta ~7,6 pontos de 329. As 8 frases de copiar/mover, mesmo indo de
# 0% a 100%, valem 8 -- ou seja, NO LIMITE do que o instrumento enxerga. Um primario
# parado NAO desmente o secundario.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do sleep 30; done
cargo build --release --bin teka || { echo "build falhou"; exit 1; }
cp -f target/release/teka.exe teka_pasta.exe || { echo "nao copiei"; exit 1; }
for marca in "arquiva" "deposita" "arrasta"; do
  grep -aq "$marca" teka_pasta.exe || { echo "BINARIO VELHO: sem '$marca'"; exit 1; }
done
echo "binario conferido: tem os moldes de pasta"

pronta() {
  [ -f "logs/pasta_s$1.log" ] || return 1
  grep -aq "ferramenta certa:" "logs/pasta_s$1.log" || return 1
  [ "logs/pasta_s$1.log" -nt teka_pasta.exe ]
}
for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then echo "=== semente ${s} — ja pronta ==="; continue; fi
  echo "=== destino de pasta, semente ${s} — $(date +%H:%M) ==="
  ./teka_pasta.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "modelos/teka_pasta_s${s}.bin" --benchmark \
    > "logs/pasta_s${s}.log" 2>&1
done
echo "=== PASTA COMPLETA — $(date +%H:%M) ==="
