#!/usr/bin/env bash
# A SUITE INTEIRA, e desta vez inteira mesmo.
#
# `--no-fail-fast`: sem isto o cargo para no primeiro binario vermelho. Foi o que
# aconteceu hoje de manha -- `memoria` caiu, `modelo` e `reforco` nunca rodaram, e
# as 7 linhas que sobraram tinham cara de suite completa. Eu li 360 e reportei
# verde; eram 371 e tres vermelhos.
#
# `--release`: em debug o teste do agente leva ~6h (ver `teka-suite-exige-release`).
#
# Log em ARQUIVO: a saida da tarefa de fundo morre com a tarefa, e ja morreu duas
# vezes hoje levando resultado junto.
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# ESPERAR OS BINARIOS DE TESTE SAIREM. No Windows um .exe em execucao fica trancado,
# e o cargo nao consegue RELINKAR por cima dele: `LNK1104, nao e possivel abrir o
# arquivo`. Nao e disputa de lock do cargo -- e o proprio executavel preso. Foi
# assim que a primeira tentativa desta suite morreu aos 4 minutos.
while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do sleep 20; done

: > suite.log
echo "### inicio $(date +%H:%M)  HEAD=$(git rev-parse --short HEAD) (+ mudancas nao commitadas)" >> suite.log
cargo test --release --no-fail-fast >> suite.log 2>&1
echo "### codigo=$? — $(date +%H:%M)" >> suite.log
{
  echo
  echo "===================== RESUMO ====================="
  grep -aE '^\s*Running|^\s*Doc-tests|test result:' suite.log | sed 's/^ *//'
  echo
  echo "vermelhos:"
  grep -aE '^\s{4}[a-z_:]+$' suite.log | sort -u | sed 's/^/  /'
  echo "total passados: $(grep -aoE '[0-9]+ passed' suite.log | awk '{s+=$1} END {print s}')"
  echo "total falhados: $(grep -aoE '[0-9]+ failed' suite.log | awk '{s+=$1} END {print s}')"
} >> suite.log
echo "### FIM $(date +%H:%M)" >> suite.log
