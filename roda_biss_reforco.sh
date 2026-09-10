#!/usr/bin/env bash
# O `reforco` ficou vermelho em QUAL commit?
#
# Suspeito nomeado antes de olhar: `983b60e`, que treinou o critico pela primeira
# vez. Ate ele, a cabeca do critico chegava ao laco de reforco CRUA -- e o limiar
# `erro_final < 0.5` foi calibrado nesse regime. Um critico que ja chega moldado
# pelo supervisionado comeca de outro lugar, e 0,5 passa a medir outra coisa.
#
# Se `983b60e^` vier VERDE e `983b60e` vermelho, esta provado.
# Se os DOIS vierem vermelhos, o suspeito e outro e eu errei de novo.
set -u
RAIZ="/c/Users/User/Projetos/Assistente/Teka-IA"
cd "$RAIZ" || exit 1
while [ ! -f reforco.log ] || ! grep -q '=== FIM' reforco.log; do sleep 20; done

for alvo in 8ca8117 983b60e; do
  arv="/c/Users/User/AppData/Local/Temp/teka_rf_${alvo}"
  echo "### ${alvo} — $(date +%H:%M)"
  git worktree add --detach "$arv" "$alvo" >/dev/null 2>&1 || { echo "  worktree falhou"; continue; }
  ( cd "$arv" && cargo test --release --test reforco -- --nocapture ) \
    > "$RAIZ/bissrf_${alvo}.log" 2>&1
  echo "  codigo=$? — $(date +%H:%M)"
  grep -aE 'erro critico 0\.|test result|panicked' "$RAIZ/bissrf_${alvo}.log" | tail -4 | sed 's/^/  /'
  git worktree remove --force "$arv" >/dev/null 2>&1
done
echo "### FIM — $(date +%H:%M)"
