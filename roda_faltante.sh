#!/usr/bin/env bash
# Fecha os 11 testes que a corrida anterior nao chegou a reportar.
#
# POR QUE UM ARQUIVO, E NAO A SAIDA DA TAREFA: da ultima vez o `cargo` escrevia
# direto para a tarefa de fundo, o laco de espera da tarefa quebrou (`[: integer
# expression expected`) e a tarefa saiu ANTES do cargo. As 7 linhas que sobraram
# pareciam a suite inteira. Log em arquivo nao depende de quem esta olhando.
cd "/c/Users/User/Projetos/Assistente/Teka-IA" || exit 1

# Espera o cargo orfao da corrida anterior sair -- dois cargo disputam o lock do
# target e o segundo so ficaria parado.
while tasklist //FI "IMAGENAME eq cargo.exe" 2>/dev/null | grep -q cargo.exe; do
  sleep 20
done

echo "=== cargo anterior saiu — $(date +%H:%M) ===" > suite_faltante.log
cargo test --release --test memoria --test modelo --test reforco >> suite_faltante.log 2>&1
echo "=== FIM — $(date +%H:%M) codigo=$? ===" >> suite_faltante.log
