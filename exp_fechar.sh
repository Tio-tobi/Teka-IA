#!/usr/bin/env bash
# ===================================================================
# A GEMEA — REGISTRADO EM 2026-09-14, ANTES DE RODAR
# ===================================================================
#
# Cria `fechar_programa`. A razao NAO e capacidade -- ela ja fechava programa pelo
# `executar_comando` com taskkill, e acertava. A razao e de REGISTRO.
#
# O QUE FOI MEDIDO (12 sementes, examples/sonda_fechar.rs, 13-14/09):
#
#     abrir  (controle)   90% certo     0% inversao
#     fechar              15% certo    71% INVERSAO
#     matar  (comando)    48% certo    23% inversao
#
#   `encerra o vscode` e `finaliza o vscode` invertem 12/12, unanime. Ate
#   `usa o taskkill pra derrubar o notepad` vira `abrir_programa` em 7/12, com a
#   palavra taskkill escrita na frase.
#
# A TESE, e ela e mecanica e nao estatistica:
#
#   Hoje o verbo nao carrega informacao nenhuma naquele espaco porque NADA depende
#   dele. `abrir_programa` e dona sozinha de "verbo + nome de programa". Criar a
#   gemea, que divide o mesmo argumento e difere SO no verbo, e o que forca o verbo
#   a virar o unico discriminador.
#
#   Por isso os moldes novos usam O MESMO POCO `PROGRAMAS`. Se o nome do programa
#   mudasse junto, ela aprenderia o nome em vez do verbo e eu teria consertado a
#   medida sem consertar a coisa.
#
# -------------------------------------------------------------------
# INSTRUMENTO
# -------------------------------------------------------------------
#
#   PRIMARIO    `ferramenta certa` nas 329 frases ORIGINAIS (sem as 10 novas),
#               pareado por semente, contra `tres_*` = 71,4%.
#               E uma medida de DANO COLATERAL: a gemea nao deveria mexer nela.
#
#   SECUNDARIO  (a) o bloco novo de 10 frases de `fechar_programa` no benchmark
#               (b) `examples/sonda_fechar.rs`, as mesmas 28 frases de antes
#
#   n = 12, FIXADO.
#
# -------------------------------------------------------------------
# O QUE FALSEIA, dito antes de rodar
# -------------------------------------------------------------------
#
#   A tese morre se QUALQUER uma acontecer:
#
#   1. `fechar` ficar abaixo de 50% na sonda. Significa que dar nome a ferramenta
#      nao bastou para o verbo virar sinal, e o problema e de outra natureza.
#
#   2. o controle `abrir` cair mais de 3 pontos. Significa que eu troquei um erro
#      por outro -- agora "abre o discord" viraria `fechar_programa`, que e
#      exatamente o mesmo defeito na direcao oposta, e PIOR, porque antes pelo menos
#      o pedido de abrir funcionava.
#
#   3. o primario cair mais de 2 pontos. Uma ferramenta nova dilui o sorteador e
#      rouba massa das outras 20; se o preco for esse, nao vale.
#
#   A n.2 e a que eu mais temo, e e a razao de o controle estar na sonda desde o
#   primeiro dia.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

while tasklist //FI "IMAGENAME eq teka_tres.exe" 2>/dev/null | grep -q teka_tres; do sleep 60; done
while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do sleep 30; done

cargo build --release --bin teka || { echo "build falhou"; exit 1; }
if ! cmp -s target/release/teka.exe teka_fechar.exe; then
  cp -f target/release/teka.exe teka_fechar.exe || { echo "nao copiei"; exit 1; }
  echo "binario novo copiado"
else
  echo "binario identico ao anterior: mtime preservado"
fi

# A trava contra binario velho. Marca que SO existe no molde novo -- e a descricao
# da ferramenta no registro, que e o que a guarda do `ler_imagem` deveria ter
# conferido em 12/09 e nao conferiu (ela grepou uma palavra que aparecia em
# comentario de arquivo embutido, e custou 4 horas de falso positivo).
for marca in "cansei do" "da um fim no" "fecha um programa que esta aberto"; do
  grep -aq "$marca" teka_fechar.exe || { echo "BINARIO INCOMPLETO: falta '$marca'"; exit 1; }
done
echo "binario conferido: a gemea esta nele"

regime() { grep -aoE '[0-9]+ exemplos de treino[^|]*\|[^|]*\|[^0-9]*[0-9]+ params' "$1" | head -1; }
REGIME_DO_BRACO=""
for f in logs/fechar_s*.log; do
  [ -f "$f" ] || continue
  grep -aq "ferramenta certa:" "$f" || continue
  REGIME_DO_BRACO="$(regime "$f")"
  [ -n "$REGIME_DO_BRACO" ] && break
done
[ -n "$REGIME_DO_BRACO" ] && echo "regime do braco: $REGIME_DO_BRACO"

pronta() {
  local f="logs/fechar_s$1.log"
  [ -f "$f" ] || return 1
  grep -aq "ferramenta certa:" "$f" || return 1
  [ -z "$REGIME_DO_BRACO" ] && return 0
  [ "$(regime "$f")" = "$REGIME_DO_BRACO" ]
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then echo "=== semente ${s} — ja pronta ==="; continue; fi
  echo "=== gemea, semente ${s} — $(date +%H:%M) ==="
  ./teka_fechar.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "modelos/teka_fechar_s${s}.bin" --benchmark \
    > "logs/fechar_s${s}.log" 2>&1
done
echo "=== GEMEA COMPLETA — $(date +%H:%M) ==="
