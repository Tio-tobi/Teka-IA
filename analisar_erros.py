# -*- coding: utf-8 -*-
"""Onde a Teka erra, agrupado — nao quanto, mas O QUE.

`medir_john.py` da o placar. Este da o diagnostico: para cada falha, o que ela
respondeu e o que deveria. Sem isso a conversa sobre "o que falta" vira opiniao.

    python analisar_erros.py <modelo.bin>
"""
import collections
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unicodedata

EXE = os.path.join(".", "target", "release", "teka.exe")
FONTE = os.path.join("dados", "frases_john_bruto.txt")
SEM_ARGUMENTO = {"hora", "memoria", "disco", "perguntar"}
CMDS = ["ipconfig", "tasklist", "dir", "hostname", "echo", "ver", "date",
        "whoami", "ping", "cls"]
PASTAS = ["downloads", "documentos", "projetos", "dados", "src", "temp",
          "desktop", "windows", "system32"]


def sem_acento(t):
    t = unicodedata.normalize("NFD", t.lower())
    return "".join(c for c in t if unicodedata.category(c) != "Mn")


def carregar():
    secoes, atual = {}, None
    for ln in io.open(FONTE, encoding="utf-8"):
        ln = ln.rstrip("\n")
        if ln.startswith("## "):
            atual = ln[3:].strip()
            secoes[atual] = []
        elif ln.strip() and not ln.startswith("#") and atual:
            secoes[atual].append(ln.strip())
    return secoes


def tem_alvo(sec, frase):
    n = sem_acento(frase)
    if sec == "escrever_arquivo":
        return bool(re.search(r"[\w-]+\.(txt|json|md|csv|log|ini)", n)) and (
            '"' in frase or "'" in frase)
    if sec == "calcular":
        return bool(re.search(r"\d", n))
    if sec == "executar_comando":
        return any(c in n for c in CMDS)
    if re.search(r"[\w-]+\.(txt|json|md|csv|log|rs|toml|pdf|ini|py)", n):
        return True
    return bool(re.search(r"\b(" + "|".join(PASTAS) + r")\b", n))


def acao(exe, modelo, frase):
    r = subprocess.run([exe, "agente", "--carregar", modelo, "--pedido", frase,
                        "--sem-memoria"],
                       capture_output=True, text=True, encoding="utf-8",
                       errors="replace")
    m = re.search(r'"acao"\s*:\s*"([a-z_]+)"', r.stdout or "")
    return m.group(1) if m else "?"


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    tmp = os.path.join(tempfile.gettempdir(), "teka_erros.bin")
    shutil.copy(sys.argv[1], tmp)

    confusao = collections.Counter()   # (esperado, veio) -> n
    exemplos = collections.defaultdict(list)
    por_grupo = collections.Counter()

    for sec, frases in carregar().items():
        for f in frases:
            if sec in SEM_ARGUMENTO:
                grupo, esperado = "sem argumento", sec
            elif tem_alvo(sec, f):
                grupo, esperado = "com alvo", sec
            else:
                grupo, esperado = "sem alvo", "perguntar"
            veio = acao(EXE, tmp, f)
            if veio != esperado:
                confusao[(esperado, veio)] += 1
                por_grupo[grupo] += 1
                if len(exemplos[(esperado, veio)]) < 3:
                    exemplos[(esperado, veio)].append(f)

    total = sum(confusao.values())
    print("\n%d erros em 500 frases\n" % total)
    print("por grupo:")
    for g, n in por_grupo.most_common():
        print("   %-16s %3d" % (g, n))

    print("\nas 12 confusoes mais frequentes:")
    print("   %-18s -> %-18s %4s" % ("esperado", "veio", "n"))
    print("   " + "-" * 46)
    for (esp, veio), n in confusao.most_common(12):
        print("   %-18s -> %-18s %4d" % (esp, veio, n))
        for f in exemplos[(esp, veio)]:
            print("      %s" % f.encode("ascii", "replace").decode("ascii"))
    os.remove(tmp)
    return 0


if __name__ == "__main__":
    sys.exit(main())
