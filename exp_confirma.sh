#!/usr/bin/env bash
# ===================================================================
# CONFIRMACAO: o -2,33 na falsa abstencao e real, ou foi pos-hoc?
# ===================================================================
#
# O QUE ACONTECEU (2026-09-05, sementes 7-18):
#
#   instrumento REGISTRADO (falsa acao sobre `perguntar`)  +0,00   t = 0,00
#   benchmark de 150                                       +1,50   t = 0,98
#   falsa abstencao — POS-HOC                              -2,33   t = -2,14
#
# O erro foi de metodo: os 62 moldes foram para as ferramentas REAIS, ou seja
# atacam "frase real que ela abstem", e eu registrei o medidor da direcao
# oposta. Pre-registrar nao protege se o instrumento nao aponta para a mudanca.
#
# E a predicao pontual FALHOU: `resgata`(8->8), `seleciona`(11->11),
# `reune`(12->12), `cade`(8->10). Nenhuma das frases que motivaram a mudanca
# se moveu. O sinal veio de outro lugar.
#
# -------------------------------------------------------------------
# INSTRUMENTO, REGISTRADO ANTES E NA DIRECAO CERTA
# -------------------------------------------------------------------
#
#   FALSA ABSTENCAO: frase de ferramenta REAL respondida com `perguntar`,
#   contada por semente sobre as 122 frases reais do benchmark.
#
#   Se os 62 moldes funcionam, ESTE numero cai. Se ele nao cair, a hipotese
#   da variedade de verbo morre.
#
#   base observada em 7-18:  ctrl 9,67   vb 7,33
#
#   O benchmark de 150 entra so para confirmar que nada quebrou em volta.
#
# -------------------------------------------------------------------
# DESENHO
# -------------------------------------------------------------------
#
#   sementes 19 a 30 — NOVAS. Confirmar no mesmo dado que gerou a hipotese
#   nao confirma nada.
#
#   dois bracos do MESMO codigo, so os 62 moldes mudando:
#     teka_ctrl.exe   dados.rs de 6f2bca8 (sem os verbos)
#     teka_vb.exe     dados.rs de HEAD    (com os verbos)
#   Verificado por conteudo, nao por data: `cochicha`, `desencava` e
#   `rastela` aparecem 1x no vb e 0x no ctrl.
#
#   ALTERNADO por semente (ctrl_19, vb_19, ctrl_20, vb_20, ...). Se a maquina
#   ficar lenta no meio da noite, os dois bracos pagam igual. As corridas
#   anteriores rodavam um braco inteiro e depois o outro, o que deixa a
#   diferenca vulneravel a qualquer deriva.
#
# -------------------------------------------------------------------
# PODER — a limitacao dita antes, nao depois
# -------------------------------------------------------------------
#
#   O desvio pareado medido foi 3,77. Com n=12, o efeito detectavel a 80% de
#   poder e ~3,2 — e o observado foi 2,33.
#
#   ENTAO: um resultado nulo aqui NAO enterra a hipotese, so descarta efeito
#   grande. Um resultado positivo, com o instrumento certo e sementes novas,
#   vale. Assimetria conhecida de antemao.
#
#   n FIXADO EM 12 POR BRACO. Nao ler parcial, nao estender depois de olhar.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# RETOMAVEL, para o John poder desligar o PC no meio.
#
# Uma corrida so conta como pronta se o LOG tem a linha final do benchmark. O
# `.bin` sozinho nao serve: uma corrida interrompida deixa arquivo pela metade,
# e ele seria pulado como se estivesse bom — trocando 12 pares por 11 sem que
# ninguem visse.
pronta() {
  [ -f "cf_$1_s$2.log" ] && grep -aq "ferramenta certa:" "cf_$1_s$2.log"
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  for braco in ctrl vb; do
    if pronta "$braco" "$s"; then
      echo "=== ${braco} semente ${s} — ja pronta, pulando ==="
      continue
    fi
    echo "=== ${braco} semente ${s} — $(date +%H:%M) ==="
    "./teka_${braco}.exe" agente --patcher por_palavra --epocas 12 --exemplos 16000 \
      --semente "$s" --threads 10 --saida "teka_cf_${braco}_s${s}.bin" --benchmark \
      > "cf_${braco}_s${s}.log" 2>&1
  done
done
echo "=== CONFIRMACAO COMPLETA — $(date +%H:%M) ==="
