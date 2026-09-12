#!/usr/bin/env bash
# ===================================================================
# BASE NOVA — o benchmark mudou, e todo numero anterior morreu com ele
# ===================================================================
#
# `dados/frases_teste.txt` foi de 150 para 329 frases e de 10 para 20 ferramentas
# cobertas (7c98e12). 113,42 e tudo antes dele foram medidos sobre as 150: NAO sao
# comparaveis. Sem esta corrida, nenhum experimento novo tem contra o que parear.
#
# Isto NAO e um tratamento e nao tem hipotese. E a referencia. O estado e
# exatamente o HEAD, sem mudanca nenhuma -- o `enriquecimento` fica na branch de
# proposito, justamente para nao contaminar a base.
#
# O numero que sair aqui e o novo "onde ela esta", e substitui o 113,42 na secao 1
# do ROTEIRO.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# Espera a suite soltar o lock e o executavel. No Windows um .exe em execucao fica
# trancado e o cargo nao relinka por cima -- ja matou uma corrida desta.
while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do sleep 30; done

cargo build --release --bin teka || { echo "build falhou"; exit 1; }
cp -f target/release/teka.exe teka_base.exe || { echo "nao copiei"; exit 1; }

# CONFERE que o binario tem o benchmark novo. `frases_teste.txt` entra por
# `include_str!`, entao a marca tem de estar DENTRO do executavel. Binario velho
# mediria as 150 e o resultado pareceria normal -- ja aconteceu, custou 4 horas.
for marca in "nyxara" "qual meu ip"; do
  grep -aq "$marca" teka_base.exe || { echo "BINARIO VELHO: sem '$marca'"; exit 1; }
done
echo "binario conferido: tem as 329 frases"

pronta() {
  [ -f "logs/base_s$1.log" ] || return 1
  grep -aq "ferramenta certa:" "logs/base_s$1.log" || return 1
  [ "logs/base_s$1.log" -nt teka_base.exe ]
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then echo "=== semente ${s} — ja pronta ==="; continue; fi
  echo "=== base 329 frases, semente ${s} — $(date +%H:%M) ==="
  ./teka_base.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "modelos/teka_base_s${s}.bin" --benchmark \
    > "logs/base_s${s}.log" 2>&1
done
echo "=== BASE COMPLETA — $(date +%H:%M) ==="
