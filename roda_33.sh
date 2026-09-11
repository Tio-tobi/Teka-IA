#!/usr/bin/env bash
# ===================================================================
# CONSTROI, CONFERE, E SO ENTAO RODA.
# ===================================================================
#
# A primeira tentativa desta corrida perdeu 4 HORAS medindo dado velho. Eu editei
# `dados.rs`, rodei `cargo test --lib` e `cargo run --example`, e copiei
# `target/release/teka.exe` -- que NENHUM dos dois reconstroi. O binario era de tres
# horas antes da edicao.
#
# O sintoma foi as seis primeiras sementes darem IDENTICO ao braco anterior, digito
# por digito. Mudanca de dado que nao move nada em seis sementes nao e resultado.
#
# E ja estava registrado como erro meu de uma semana antes. Por isso o passo humano
# some: o script constroi, CONFERE que o binario tem o dado novo, e so entao roda.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

cargo build --release --bin teka || { echo "build falhou"; exit 1; }
cp -f target/release/teka.exe teka_33.exe || {
  echo "nao consegui copiar -- ha uma corrida antiga segurando o arquivo?"; exit 1; }

# A CONFERENCIA. `tela_do_erro.png` so existe no poco `IMAGENS`, que entrou em
# 2fa614d. Se nao estiver no binario, ele e velho e a corrida seria lixo.
for marca in tela_do_erro.png api_key; do
  grep -aq "$marca" teka_33.exe || {
    echo "BINARIO VELHO: nao achei '$marca'. Nao vou rodar 8h em cima de dado errado."
    exit 1; }
done
echo "binario conferido: tem os pocos novos"

exec ./exp_33.sh >> tr33_corrida.log 2>&1
