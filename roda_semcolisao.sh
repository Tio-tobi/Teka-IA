#!/usr/bin/env bash
# `ARQUIVOS` tinha `foto.png` e `imagem.jpg`; `IMAGENS` e so extensao de imagem.
# Colisao direta no sinal mais forte que as duas ferramentas tem.
#
# O A/B mediu que os pocos custam 5,6 pontos de intencao (80,4 -> 74,8). Se a
# colisao explicar parte disso, tirar as duas recupera. Se nao recuperar nada, o
# custo e da mudanca de ESPACO (texto -> caminho) e nao das duas entradas.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
echo "### sem colisao de extensao — $(date +%H:%M)" > logs/semcolisao.log
cargo test --release --test agente \
  o_agente_aprende_a_escolher_ferramenta_e_argumento -- --nocapture \
  >> logs/semcolisao.log 2>&1
echo "### codigo=$? — $(date +%H:%M)" >> logs/semcolisao.log
