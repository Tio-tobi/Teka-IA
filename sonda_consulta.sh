#!/usr/bin/env bash
# Sonda dirigida: o ponteiro leva a preposicao do molde junto com o valor?
#
# O sintoma medido: "pesquisa na internet sobre fotofobia" devolvia
# `"consulta":"sobre fotofobia"`, e a API da DuckDuckGo nao acha nada com isso — o
# erro chega ao usuario como "nada encontrado".
#
# Causa: das 30 consultas do poco, 20 comecavam com palavra funcional ("o", "como",
# "quem") e NENHUMA era substantivo sozinho. O ponteiro aprendeu que consulta comeca
# com palavra curta e esticava para tras para pegar o "sobre".
#
# Os termos usados aqui NAO estao no poco de treino, de proposito. E os de
# acessibilidade que o John escreveu ficam de fora: eles sao regua.
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1
BIN=${BIN:-./teka_exp.exe}
MODELO=${1:?uso: sonda_consulta.sh <modelo.bin> [flags extras]}
shift || true

# (frase, valor esperado)
FRASES=(
  "pesquisa na internet sobre osmose|osmose"
  "consulta na web sobre gravitacao|gravitacao"
  "busca informacao sobre magnetismo|magnetismo"
  "levanta informacao sobre eletrolise|eletrolise"
  "o que a internet diz sobre pasteurizacao|pasteurizacao"
  "da uma olhada na web sobre sismologia|sismologia"
  "me atualiza sobre paleontologia|paleontologia"
  "quero saber sobre metalurgia|metalurgia"
  "o que se sabe sobre citologia|citologia"
  "procura na web informacao sobre glaciacao|glaciacao"
  "pesquisa e resume vulcanologia|vulcanologia"
  "da um google em espeleologia|espeleologia"
)

acertos=0
ferramenta_ok=0
com_prep=0
for par in "${FRASES[@]}"; do
    frase="${par%%|*}"
    esperado="${par##*|}"
    saida=$($BIN agente --carregar "$MODELO" "$@" --pedido "$frase" 2>&1)
    acao=$(echo "$saida" | grep -aoE '"acao":"[a-z_]+"' | head -1 | sed 's/.*:"//;s/"//')
    valor=$(echo "$saida" | grep -aoE '"consulta":"[^"]*"' | head -1 | sed 's/^"consulta":"//;s/"$//')
    [ "$acao" = "buscar_web" ] && ferramenta_ok=$((ferramenta_ok+1))
    [ "$valor" = "$esperado" ] && acertos=$((acertos+1))
    case "$valor" in
        sobre*|de\ *|em\ *|na\ *) com_prep=$((com_prep+1)) ;;
    esac
done

echo "$(basename "$MODELO"): ferramenta ${ferramenta_ok}/${#FRASES[@]} | argumento exato ${acertos}/${#FRASES[@]} | com preposicao ${com_prep}"
