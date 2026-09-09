#!/usr/bin/env bash
# ===================================================================
# TIRAR A CONTRADICAO DO FORA-DE-ESCOPO
# ===================================================================
#
# Quatro exemplos ensinados como `perguntar` eram pedidos que ela ATENDE:
#
#   "aumenta o volume"      -> aumentar_volume   (gatilho LITERAL da tabela)
#   "toca uma musica ai"    -> tocar_faixa
#   "poe um som pra tocar"  -> tocar_faixa
#   "quero ouvir podcast"   -> tocar_faixa
#
# O primeiro e o caso puro: a MESMA string estava no poco de `atalho` rotulada
# `atalho` e no fora-de-escopo rotulada `perguntar`. Dois rotulos, uma frase.
#
# Trocados um por um por dominios que ela nao tem (uber, luz da sala, ar
# condicionado, mesa em restaurante). O tamanho do poco NAO muda: o que muda e
# quais frases, nao quantas.
#
# -------------------------------------------------------------------
# INSTRUMENTO, registrado em 07/09 e mantido
# -------------------------------------------------------------------
#
#   frases de `perguntar` e `hora` que viram `atalho`, por semente,
#   EXCLUINDO a linha 117 (naquela o modelo esta certo, o gabarito e velho)
#
#   base    20 erros em 12 sementes = 1,67 por semente
#   se a mudanca funcionar, ESTE numero cai
#
#   o benchmark de 150 entra so para confirmar que nada quebrou em volta
#
# -------------------------------------------------------------------
# DESENHO
# -------------------------------------------------------------------
#
#   Mesmas 12 sementes (19-30), contra `ct_20_*` que ja esta no disco. n=12,
#   FIXADO. Nao ler parcial, nao estender depois de olhar.
#
#   RESSALVA dita antes: o -2,58 que motivou isto NAO era significativo, e
#   consertar efeito nao significativo e a receita de perseguir ruido. O que
#   sustenta o passo sao os 25 erros concretos numa forma unica -- mecanismo
#   OBSERVADO, nao inferido de um p. Se o instrumento nao se mover, a hipotese
#   morre, e nao se procura outro numero depois.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# RETOMAVEL: so conta como pronta a corrida cujo log chegou ao fim.
pronta() {
  [ -f "oos_s$1.log" ] && grep -aq "ferramenta certa:" "oos_s$1.log"
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then
    echo "=== semente ${s} — ja pronta, pulando ==="
    continue
  fi
  echo "=== 20 ferramentas, semente ${s} — $(date +%H:%M) ==="
  ./teka_oos.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "teka_oos_s${s}.bin" --benchmark \
    > "oos_s${s}.log" 2>&1
done
echo "=== CUSTO COMPLETO — $(date +%H:%M) ==="
