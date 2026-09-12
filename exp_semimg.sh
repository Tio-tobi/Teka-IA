#!/usr/bin/env bash
# ===================================================================
# `ler_imagem` FORA — REGISTRADO EM 2026-09-12, ANTES DE RODAR
# ===================================================================
#
# Ela saiu por NAO FUNCIONAR (78e3f8a), nao por custo. Mas sair do registro devolve
# espaco de classificacao, e isso e medivel.
#
#   PRIMARIO   `ferramenta certa` de 329, pareado por semente.
#              Base: o braco `pasta_*` (21... nao: 22 ferramentas, com a imagem).
#              A comparacao e contra o braco QUE ESTIVER FECHADO quando esta rodar.
#
#   n = 12, FIXADO.
#
# -------------------------------------------------------------------
# A EXPECTATIVA, E O QUANTO ELA VALE
# -------------------------------------------------------------------
#
# Tirar ferramenta ajuda: menos classes, mesma capacidade. O precedente e do outro
# lado e mede a escala -- 20 -> 22 custou -2,75, entao 22 -> 21 devolveria ~1,4 se
# o custo fosse linear.
#
# E 1,4 esta ABAIXO do chao do instrumento: o desvio entre sementes na regua de 329
# mediu 3,64 pp, e com n=12 isso detecta ~7,6 pontos de 329.
#
# Ou seja: ESTE EXPERIMENTO PROVAVELMENTE NAO CONCLUI NADA, e isso esta escrito
# antes de rodar. Ele vale por dois motivos mesmo assim:
#
#   1. GUARDA. Tirar ferramenta nao pode PIORAR. Se o numero cair muito, algo que eu
#      nao previ quebrou junto -- e a `ler_imagem` saiu levando poco e codigo.
#   2. O benchmark tem ZERO frases de `ler_imagem`, entao o efeito so pode vir de
#      espaco de classificacao. E efeito puro, sem mistura.
#
# Um resultado parado aqui NAO desmente a remocao: ela se justifica por a ferramenta
# nao executar, e isso ja esta medido.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

while tasklist //FI "IMAGENAME eq teka_pasta.exe" 2>/dev/null | grep -q teka_pasta; do sleep 60; done
while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do sleep 30; done

cargo build --release --bin teka || { echo "build falhou"; exit 1; }
cp -f target/release/teka.exe teka_semimg.exe || { echo "nao copiei"; exit 1; }
# CONFERE que e o binario certo: sem `ler_imagem` e com os moldes de pasta.
grep -aq 'ler_imagem' teka_semimg.exe && { echo "BINARIO VELHO: ainda tem ler_imagem"; exit 1; }
grep -aq 'arquiva' teka_semimg.exe || { echo "BINARIO VELHO: sem os moldes de pasta"; exit 1; }
echo "binario conferido: 21 ferramentas, com os moldes de pasta"

pronta() {
  [ -f "logs/semimg_s$1.log" ] || return 1
  grep -aq "ferramenta certa:" "logs/semimg_s$1.log" || return 1
  [ "logs/semimg_s$1.log" -nt teka_semimg.exe ]
}
for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then echo "=== semente ${s} — ja pronta ==="; continue; fi
  echo "=== sem ler_imagem, semente ${s} — $(date +%H:%M) ==="
  ./teka_semimg.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "modelos/teka_semimg_s${s}.bin" --benchmark \
    > "logs/semimg_s${s}.log" 2>&1
done
echo "=== SEM IMAGEM COMPLETA — $(date +%H:%M) ==="
