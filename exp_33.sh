#!/usr/bin/env bash
# ===================================================================
# O POCO DAS DUAS FERRAMENTAS NOVAS — REGISTRADO EM 2026-09-10, ANTES DE RODAR
# ===================================================================
#
# `ler_imagem` e `buscar_no_conteudo` passaram nove dias aprendendo caminho a partir
# do poco de recado, porque `valor_para` nao tinha braco para elas. Consertado em
# `2fa614d`: pocos `IMAGENS` e `PADROES`, rede por nome de parametro, e um teste que
# cobra a propriedade.
#
# -------------------------------------------------------------------
# INSTRUMENTO
# -------------------------------------------------------------------
#
#   PRIMARIO   `ferramenta certa` de 150, pareado por semente.
#              Base: braco `tr22_*` (22 ferramentas, poco quebrado): 113,42.
#              `frases_teste.txt` NAO mudou -- mesma regua dos dois lados.
#
#              A base E PAREAVEL, e isso foi MEDIDO e nao suposto: a separacao das
#              cabecas (7ea0241) e mudanca de arquitetura, e o controle deu 117/150
#              na semente 19 nos dois binarios, identico ate a decima em intencao,
#              argumento e ponta-a-ponta.
#
#   SECUNDARIO erro por ferramenta ESPERADA, contra o mesmo braco.
#
#   n = 12, FIXADO. Nao ler parcial. `analisa_22.py` recusa por construcao.
#
# -------------------------------------------------------------------
# A EXPECTATIVA, DITA ANTES, E O QUE A FALSEIA
# -------------------------------------------------------------------
#
# O benchmark de 150 NAO TEM uma frase sequer sobre imagem ou busca em conteudo.
# Entao o efeito DIRETO desta mudanca sobre ele e zero, por construcao.
#
# O que pode subir e a familia de CAMINHO DE ARQUIVO -- `escrever_arquivo`,
# `listar_pasta`, `ler_arquivo`, `procurar_arquivo` -- porque duas ferramentas
# param de disputar aquele espaco com argumento que nao parece caminho.
#
# FALSEAMENTO: se subir e NAO for na familia de caminho, o mecanismo que escrevi
# esta errado. Registro isso sabendo que meu placar de prever superficie e de
# QUATRO tentativas e ZERO acertos (ver ROTEIRO, 2026-09-10). Por isso o primario e
# a medida AMPLA, e a familia entra como secundario e EXPLICACAO -- nunca como o
# numero que decide.
#
# E o caso de nao mover tambem informa: querria dizer que o poco quebrado custava
# menos do que a mecanica sugere, e que os -2,75 da guarda vem de outro lugar.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# Pronta = log chegou ao fim E foi feito por ESTE binario.
#
# A segunda metade nao estava aqui e quase custou caro: quando a primeira tentativa
# desta corrida rodou com binario velho, eu apaguei os logs -- mas a corrida antiga
# seguia viva e escreveu mais dois DEPOIS. No dia seguinte o `pronta()` os teria
# pulado como bons, e o braco sairia com 2 sementes de dado velho misturadas.
#
# Retomabilidade que aceita log mais antigo que o binario nao e retomabilidade, e
# contaminacao silenciosa.
pronta() {
  [ -f "tr33_s$1.log" ] || return 1
  grep -aq "ferramenta certa:" "tr33_s$1.log" || return 1
  [ "tr33_s$1.log" -nt teka_33.exe ]
}

for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then echo "=== semente ${s} — ja pronta, pulando ==="; continue; fi
  echo "=== poco consertado, semente ${s} — $(date +%H:%M) ==="
  ./teka_33.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "teka_tr33_s${s}.bin" --benchmark \
    > "tr33_s${s}.log" 2>&1
done
echo "=== POCO COMPLETO — $(date +%H:%M) ==="
