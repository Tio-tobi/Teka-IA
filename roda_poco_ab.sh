#!/usr/bin/env bash
# O conserto dos pocos foi LIQUIDO positivo? A/B no mesmo teste.
#
# O teste do agente caiu de ~79,3% para 74,8% de intencao em frases ineditas, e a
# suite estava verde antes de `2fa614d`. Mecanismo suspeito: `ler_imagem` agora
# aprende `foto.jpg`, que PARECE `notas.md` do `ler_arquivo`. Pode ter trocado um
# problema (argumento de recado) por outro (colisao com a vizinha).
#
# Mesma semente, mesmo tudo. So os pocos mudam.
set -u
RAIZ="/c/Users/User/Projetos/Assistente/Teka-IA"
SEM="/c/Users/User/AppData/Local/Temp/teka_sempoco"
: > "$RAIZ/logs/poco_ab.log"
for lado in "COM os pocos novos:$RAIZ" "SEM os pocos (revertido):$SEM"; do
  nome="${lado%%:*}"; dir="${lado##*:}"
  echo "### $nome — $(date +%H:%M)" >> "$RAIZ/logs/poco_ab.log"
  ( cd "$dir" && cargo test --release --test agente \
      o_agente_aprende_a_escolher_ferramenta_e_argumento -- --nocapture ) \
    >> "$RAIZ/logs/poco_ab.log" 2>&1
  echo "### codigo=$? — $(date +%H:%M)" >> "$RAIZ/logs/poco_ab.log"
done
echo "### FIM — $(date +%H:%M)" >> "$RAIZ/logs/poco_ab.log"
