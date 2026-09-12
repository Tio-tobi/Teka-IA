#!/usr/bin/env bash
# CONTROLE POSITIVO antes de gastar 8h.
#
# A separacao das cabecas (7ea0241) e mudanca de ARQUITETURA. Se ela mover o
# benchmark, a base `tr22_*` (113,42) deixa de ser pareavel e todo experimento de
# dado daqui pra frente precisa de base nova -- 12 sementes, ~8h.
#
# O argumento diz que nao move: `Teka::new` vem ANTES das cabecas, `emb_slot` e
# `emb_byte` sao preenchidos antes do literal, e `auto` e o ULTIMO a consumir rng.
# Nada e inicializado depois dele, entao o fluxo de sorteio de todo o resto e
# identico. E o critico nao propaga gradiente para o tronco (backward com `None`).
#
# Argumento nao e medida. `tr22_s19` deu 117/150 no binario antigo. Se der 117 de
# novo, a base serve. Se nao der, descobri por 40 min em vez de por 8h.
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
./teka_32.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
  --semente 19 --threads 10 --saida teka_ctrl_s19.bin --benchmark \
  > logs/ctrl_s19.log 2>&1
echo "=== FIM codigo=$? — $(date +%H:%M)" >> logs/ctrl_s19.log
