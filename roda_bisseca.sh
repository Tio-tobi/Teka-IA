#!/usr/bin/env bash
# DE QUAL COMMIT VEIO O VERMELHO?
#
# `a_memoria_recupera_o_episodio_certo_pela_leitura_do_pedido` falha em HEAD com
# "que horas sao" (cos 0,158) no lugar de "quanto de memoria esta em uso".
#
# Duas suspeitas, e o mecanismo aponta so para a segunda:
#   e522bdc  buscar_web   -- mexeu no CORPO do executar; o teste nao roda ferramenta
#   033d533  20 -> 22     -- mexeu no `gerar`, e o teste treina em cima do `gerar`
#
# Arvore separada de proposito: a guarda esta rodando com `teka_22.exe` nesta pasta,
# e checkout no lugar seria mexer no chao de quem esta de pe.
set -u
RAIZ="/c/Users/User/Projetos/Assistente/Teka-IA"
cd "$RAIZ" || exit 1

for alvo in e355765 aeacf77; do
  arv="/c/Users/User/AppData/Local/Temp/teka_biss_${alvo}"
  echo "### ${alvo} — $(date +%H:%M)"
  git worktree add --detach "$arv" "$alvo" >/dev/null 2>&1 || { echo "  worktree falhou"; continue; }
  ( cd "$arv" && cargo test --release --test memoria \
      a_memoria_recupera_o_episodio_certo_pela_leitura_do_pedido -- --nocapture ) \
    > "$RAIZ/biss_${alvo}.log" 2>&1
  echo "  codigo=$? — $(date +%H:%M)"
  grep -aE 'recuperou|melhor epoca|test result' "$RAIZ/biss_${alvo}.log" | sed 's/^/  /'
  git worktree remove --force "$arv" >/dev/null 2>&1
done
echo "### FIM — $(date +%H:%M)"
