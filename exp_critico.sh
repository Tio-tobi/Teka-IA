#!/usr/bin/env bash
# ===================================================================
# PASSO 1: ALIMENTAR O CRITICO COM A PROPRIA CORRECAO
# ===================================================================
#
# A cabeca de critico tinha ZERO referencias em `supervisionado.rs`. Ela tinha
# parametros, produzia numero, e o numero era ruido -- e o benchmark ja media
# isso e ninguem lia:
#
#   margem     melhor limiar   0.70  saldo +11
#   critico    melhor limiar  -0.30  saldo  +0     em 12 de 12 sementes
#
# Agora `Alvo::auto_critico` faz ela prever "a minha propria escolha vai estar
# certa?" -- ferramenta E argumento, alvo 0/1, no mesmo passo, sem forward extra.
#
# -------------------------------------------------------------------
# LINHA DE BASE: `oos_*`, o braco de 08/09
# -------------------------------------------------------------------
#
#   Decidido pelo John ANTES de qualquer numero. O binario do fora-de-escopo foi
#   compilado antes do critico; comparar contra `ct_20_*` misturaria as duas
#   mudancas. Contra `oos_*`, a unica diferenca e o critico.
#
#   base:  sep_critico saldo +0 (12/12)   benchmark 113,50
#
# -------------------------------------------------------------------
# INSTRUMENTO — ja existe e ja esta zerado
# -------------------------------------------------------------------
#
#   PRIMARIO    saldo do `critico` na tabela de separacao do benchmark.
#               Se o passo funcionar, ele sai do zero.
#
#   GUARDA      benchmark de 150. Treinar mais uma cabeca nao pode estragar as
#               outras.
#
#   Aqui o estreito PODE ser primario: esta e mudanca de CODIGO, com mecanismo
#   conhecido, e nao mudanca de dado. A regra de "registrar a medida ampla" nasceu
#   de duas falhas em mudanca de DADO, onde eu nao sei prever a superficie.
#
# -------------------------------------------------------------------
# REGRA DE PARADA — a porta lateral do clipping
# -------------------------------------------------------------------
#
#   O gradiente do critico NAO flui para o tronco (destacado de proposito: com ele
#   passando, a intencao caiu de 77% para 56%, medido). Mas o clipping do Adam e de
#   NORMA GLOBAL: mais gradiente na soma, escala menor, tronco andando menos.
#
#   SE o benchmark desabar ja na semente 19, o suspeito e o clipping e NAO a
#   hipotese. Conserto conhecido: tirar a cabeca de critico da norma. Isto e
#   distinto de "o efeito nao apareceu", e nao vou confundir os dois depois.
#
#   n = 12, FIXADO. Nao ler parcial (a checagem da semente 19 e a regra de parada
#   acima, e so ela).
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# RETOMAVEL: so conta como pronta a corrida cujo log chegou ao fim.
pronta() {
  [ -f "logs/cr_s$1.log" ] && grep -aq "ferramenta certa:" "logs/cr_s$1.log"
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then
    echo "=== semente ${s} — ja pronta, pulando ==="
    continue
  fi
  echo "=== 20 ferramentas, semente ${s} — $(date +%H:%M) ==="
  ./teka_cr.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "modelos/teka_cr_s${s}.bin" --benchmark \
    > "logs/cr_s${s}.log" 2>&1
done
echo "=== CUSTO COMPLETO — $(date +%H:%M) ==="
