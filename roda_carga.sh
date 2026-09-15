#!/usr/bin/env bash
# Mede os limites da Teka: vazamento, vazao, tamanho do pedido, threads, entradas.
#
# POR QUE ELE EXISTE SEPARADO DA SONDA
# ------------------------------------
# Metade das perguntas ("quanto ela aguenta") e sobre TEMPO, e tempo nao se mede com
# outro processo comendo a CPU. Em 14/09 havia um treino (`teka_fechar.exe agente
# --threads 10`) rodando ate ~1h da manha, e 10 dos 12 threads logicos estavam
# ocupados. As medidas de memoria e de limite de entrada foram tiradas assim mesmo,
# porque nao dependem de relogio; as de tempo ficaram para este script.
#
# ELE SE RECUSA A RODAR COM TREINO NO AR. E de proposito: relatar tempo contaminado
# como se fosse limpo e o pior resultado possivel. Use --mesmo-assim se voce QUER o
# numero contaminado e vai rotular como tal.
#
#   ./roda_carga.sh                    so com a maquina livre
#   ./roda_carga.sh --mesmo-assim      roda sob contencao, e marca a saida
#   ./roda_carga.sh --modelo modelos/teka_fechar_s21.bin
#
# A saida vai para logs/carga_<data>.log, com o cabecalho da condicao junto — um
# numero sem a condicao nao serve para comparar com nada depois.
set -u
cd "$(dirname "$0")" || exit 1

MODELO="modelos/teka_fechar_s19.bin"
MESMO_ASSIM=0
SEGUNDOS=300
while [ $# -gt 0 ]; do
  case "$1" in
    --mesmo-assim) MESMO_ASSIM=1 ;;
    --modelo) MODELO="$2"; shift ;;
    --segundos) SEGUNDOS="$2"; shift ;;
    *) echo "  opcao desconhecida: $1"; exit 2 ;;
  esac
  shift
done

# --- a condicao, medida e nao suposta ---
# `tasklist` porque o Git Bash do Windows nao tem `pgrep` confiavel para .exe.
CONCORRENTES=$(tasklist //FI "IMAGENAME eq teka_fechar.exe" //NH 2>/dev/null | grep -c "teka_fechar" || true)
# Qualquer outro teka_*.exe conta tambem: treino nao e so o de fechar.
OUTROS=$(tasklist //NH 2>/dev/null | grep -c "^teka_" || true)
CPUS=$(nproc 2>/dev/null || echo "?")

if [ "$CONCORRENTES" -gt 0 ] && [ "$MESMO_ASSIM" -eq 0 ]; then
  echo "  RECUSADO: ha $CONCORRENTES teka_fechar.exe rodando."
  echo "  Toda medida de tempo sairia contaminada. Espere o treino acabar,"
  echo "  ou rode com --mesmo-assim se voce vai rotular o numero como contaminado."
  exit 1
fi

mkdir -p logs
LOG="logs/carga_$(date +%Y%m%d_%H%M%S).log"
SONDA="./target/release/examples/sonda_carga.exe"

if [ ! -x "$SONDA" ]; then
  echo "  compilando a sonda (release: em debug o numero nao vale nada)"
  cargo build --release --example sonda_carga || exit 1
fi

{
  echo "=== CONDICAO DA MEDIDA ==="
  echo "quando            : $(date '+%Y-%m-%d %H:%M:%S')"
  echo "modelo            : $MODELO"
  echo "cpus logicas      : $CPUS"
  echo "teka_fechar vivos : $CONCORRENTES"
  echo "outros teka_*     : $OUTROS"
  if [ "$CONCORRENTES" -gt 0 ]; then
    echo "!!! CONTAMINADO: havia treino competindo. TODO numero de tempo abaixo"
    echo "!!! e um piso pessimista, nao a capacidade da maquina."
  else
    echo "limpo             : nenhum treino da Teka no ar"
  fi
  echo "processos pesados no momento:"
  tasklist //NH 2>/dev/null | sort -k5 -n -r | head -8
  echo
} | tee "$LOG"

for MODO in contabilidade limites tamanho threads; do
  {
    echo
    echo "############ $MODO ############"
  } | tee -a "$LOG"
  "$SONDA" "$MODO" "$MODELO" 5000 2>&1 | tee -a "$LOG"
done

{
  echo
  echo "############ folego ($SEGUNDOS s) ############"
} | tee -a "$LOG"
"$SONDA" folego "$MODELO" "$SEGUNDOS" 2>&1 | tee -a "$LOG"

echo
echo "  pronto: $LOG"
