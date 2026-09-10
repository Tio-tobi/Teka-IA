#!/usr/bin/env bash
# ===================================================================
# QUANTO CUSTAM AS TRES FERRAMENTAS DA PONTE?
# ===================================================================
#
# `buscar_no_conteudo` e `ler_imagem` entraram em 09/09, vindas da ponte do
# Harness. O registro foi de 20 para 22.
#
# Eram TRES. `editar_arquivo` saiu antes de chegar aqui, e saiu por medicao: com ela,
# o teste de integracao dava 85,0% de argumento em frases ineditas contra um limiar
# de 88%; sem ela, passa. Era a UNICA de tres argumentos do registro inteiro, e
# argumento multiplo e o lugar dificil desta casa.
#
# A PERGUNTA E A DE SEMPRE, e nao a que parece: nao e "as tres funcionam?" -- e
# "as outras 20 PIORARAM?". O benchmark de 150 nao tem uma frase sequer sobre
# grep, edicao ou imagem, entao ele nao mede as novas. Ele mede o ESTRAGO.
#
# E medir o estrago e o que ninguem fez quando o registro cresceu de 10 para 19:
# caiu de 108,3 para 107,0 e so se descobriu depois.
#
# -------------------------------------------------------------------
# INSTRUMENTO, REGISTRADO ANTES
# -------------------------------------------------------------------
#
#   PRIMARIO   `ferramenta certa` de 150, pareado por semente.
#              `frases_teste.txt` NAO mudou -- a regua e a mesma dos dois lados.
#              base (braco `cr_*`, com o critico): 116,17
#
#   O TESTE DE INTEGRACAO JA E UMA GUARDA, e ele ja falou: com as duas que ficaram,
#   passa. Esta corrida mede o mesmo em escala de verdade, com 12 sementes.
#
#   Se as tres classes novas atrapalham, ESTE numero cai.
#
#   SECUNDARIO onde o erro caiu, por ferramenta esperada. E o que separa "custo
#              difuso" de "uma ferramenta especifica foi roubada" -- e a suspeita
#              nomeada de antemao e `procurar_arquivo`, que e a vizinha de
#              `buscar_no_conteudo`.
#
# -------------------------------------------------------------------
# A SUSPEITA, DITA ANTES DE OLHAR
# -------------------------------------------------------------------
#
#   `buscar_no_conteudo` e `procurar_arquivo` fazem coisas parecidas e a Teka
#   aprende SUPERFICIE, nao conceito. Se o custo aparecer, o lugar mais provavel e
#   `procurar_arquivo` -- e o conserto conhecido e a marca de conteudo, que ja esta
#   em 16 de 16 frases do molde novo.
#
#   `ler_imagem` eu NAO espero que atrapalhe: um argumento so, e a superficie dela
#   (imagem/foto/print junto da extensao) nao colide com nada. Se o custo vier dali,
#   minha leitura do mecanismo estava errada.
#
#   Se o custo aparecer em OUTRO lugar, minha explicacao estava errada, e vale
#   registrar como mais um caso de eu nao saber prever qual superficie uma mudanca
#   de dado toca (ver `teka-registrar-a-medida-ampla`: dois pontos, zero acertos).
#
# -------------------------------------------------------------------
# LINHA DE BASE: `cr_*`, o braco do critico
# -------------------------------------------------------------------
#
#   E o estado imediatamente anterior a esta mudanca. Comparar contra `oos_*`
#   misturaria o critico junto.
#
#   n = 12, FIXADO. Nao ler parcial.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# RETOMAVEL: so conta como pronta a corrida cujo log chegou ao fim.
pronta() {
  [ -f "tr22_s$1.log" ] && grep -aq "ferramenta certa:" "tr22_s$1.log"
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then
    echo "=== semente ${s} — ja pronta, pulando ==="
    continue
  fi
  echo "=== 20 ferramentas, semente ${s} — $(date +%H:%M) ==="
  ./teka_22.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "teka_tr22_s${s}.bin" --benchmark \
    > "tr22_s${s}.log" 2>&1
done
echo "=== CUSTO COMPLETO — $(date +%H:%M) ==="
