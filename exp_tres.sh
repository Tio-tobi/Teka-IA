#!/usr/bin/env bash
# ===================================================================
# TRES FORMAS DE UMA VEZ — REGISTRADO EM 2026-09-12, ANTES DE RODAR
# ===================================================================
#
# Tres consertos de dado, cada um diagnosticado por medicao e cada um mirando uma
# familia DIFERENTE. Juntos num braco so porque o secundario atribui sozinho:
#
#   TEXTOS de 7 para 45        -> escrever_arquivo   (52,4%)
#   +15 verbos de ver          -> listar_pasta       (78,0%)
#   +10 marcadores de conteudo -> buscar_no_conteudo (24,0%)
#   +20 imperativos vagos      -> perguntar          (54,5%)
#
# Separado seriam 24 horas. Junto sao 8, e famílias distintas nao se confundem na
# leitura. Se duas mexerem na mesma, ai sim eu nao saberia qual foi.
#
# -------------------------------------------------------------------
# INSTRUMENTO
# -------------------------------------------------------------------
#
#   PRIMARIO   `ferramenta certa` de 329, pareado por semente, contra `pasta_*`
#              REPONTUADO com os rotulos de hoje: 219,00 (66,6%).
#              Nao contra o numero original de 217,17 -- aquele e de outro rotulo.
#
#   SECUNDARIO acerto das QUATRO familias acima, uma a uma. E o que separa
#              "funcionou" de "qual funcionou".
#
#   n = 12, FIXADO.
#
# -------------------------------------------------------------------
# O QUE FALSEIA CADA UMA, dito antes
# -------------------------------------------------------------------
#
#   perguntar          e o maior bloco (131 frases) e o pior numero. Se ele nao
#                      subir, a explicacao do imperativo vago esta errada -- e ela
#                      e a mais bem fundamentada das quatro: medido que 13 das 14
#                      frases roubadas por `atalho` nao casam gatilho nenhum, entao
#                      e a cabeca de intencao, nao a tabela.
#
#   escrever_arquivo   `TEXTOS` era o poco mais fino (7 valores) e a forma que
#                      faltava era determinante ("a nota X"). Precedente medido:
#                      COMANDOS de 6 para 62 tirou `executar_comando` de 37 erros.
#
#   buscar_no_conteudo nao faltava marcador, faltava VARIEDADE dele. Medido: ela
#                      erra "procura a palavra senha NOS arquivos" tendo o molde
#                      "procura pela palavra {0} DENTRO DOS arquivos" -- quase
#                      identicas. Se nao subir, ela memoriza string e nao aprende
#                      conceito, e o conserto e outro.
#
#   listar_pasta       ja esta em 78%, entao sobra pouco. Entra de carona.
#
# O PRIMARIO PODE NAO MOVER mesmo com o secundario bom: sao quatro familias de 14 a
# 131 frases, e `perguntar` sozinho pesa 131 de 329. So ele decide o primario.
# ===================================================================
set -u
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

while tasklist //FI "IMAGENAME eq teka_semimg.exe" 2>/dev/null | grep -q teka_semimg; do sleep 60; done
while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do sleep 30; done

cargo build --release --bin teka || { echo "build falhou"; exit 1; }
# SO COPIA SE MUDOU. `cp -f` atualiza o mtime mesmo com conteudo identico, e o
# `pronta()` compara o log com o mtime do binario -- entao recopiar a cada retomada
# fazia TODA semente parecer nao-pronta. A trava contra binario velho se derrotava
# a si mesma: em 13/09 ela mandou refazer 7 sementes ja prontas.
if ! cmp -s target/release/teka.exe teka_tres.exe; then
  cp -f target/release/teka.exe teka_tres.exe || { echo "nao copiei"; exit 1; }
  echo "binario novo copiado"
else
  echo "binario identico ao anterior: mtime preservado"
fi
# Confere as QUATRO mudancas, uma marca de cada. Marca que so existe no molde novo.
for marca in "o recado do joao" "esmiuca a pasta" "grep de" "ajeita aquele negocio"; do
  grep -aq "$marca" teka_tres.exe || { echo "BINARIO INCOMPLETO: falta '$marca'"; exit 1; }
done
echo "binario conferido: as quatro formas estao nele"

# O REGIME, e nao o mtime.
#
# A versao anterior exigia que o log fosse mais NOVO que o binario -- para nao
# misturar log velho com dado novo. Mas o script recopia o binario a cada retomada, e
# `cp` atualiza o mtime mesmo com conteudo identico. A trava se derrotava a si mesma:
# em 13/09 ela mandou refazer 7 sementes prontas e sobrescreveu a primeira antes de
# eu perceber.
#
# Cada log diz o proprio regime na terceira linha:
#
#     17805 exemplos de treino, 6175 de validacao | 21 ferramentas | 1615866 params
#
# Isso e ASSINATURA DE CONTEUDO: muda quando o registro muda e muda quando os moldes
# mudam. A primeira semente que fecha define o regime do braco; as outras tem de
# bater com ela. Log de outro regime nao conta como pronto.
regime() { grep -aoE '[0-9]+ exemplos de treino[^|]*\|[^|]*\|[^0-9]*[0-9]+ params' "$1" | head -1; }

REGIME_DO_BRACO=""
for f in logs/tres_s*.log; do
  [ -f "$f" ] || continue
  grep -aq "ferramenta certa:" "$f" || continue
  REGIME_DO_BRACO="$(regime "$f")"
  [ -n "$REGIME_DO_BRACO" ] && break
done
[ -n "$REGIME_DO_BRACO" ] && echo "regime do braco: $REGIME_DO_BRACO"

pronta() {
  local f="logs/tres_s$1.log"
  [ -f "$f" ] || return 1
  grep -aq "ferramenta certa:" "$f" || return 1
  [ -z "$REGIME_DO_BRACO" ] && return 0
  [ "$(regime "$f")" = "$REGIME_DO_BRACO" ]
}
for s in 19 20 21 22 23 24 25 26 27 28 29 30; do
  if pronta "$s"; then echo "=== semente ${s} — ja pronta ==="; continue; fi
  echo "=== tres formas, semente ${s} — $(date +%H:%M) ==="
  ./teka_tres.exe agente --patcher por_palavra --epocas 12 --exemplos 16000 \
    --semente "$s" --threads 10 --saida "modelos/teka_tres_s${s}.bin" --benchmark \
    > "logs/tres_s${s}.log" 2>&1
done
echo "=== TRES FORMAS COMPLETA — $(date +%H:%M) ==="
