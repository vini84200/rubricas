---
name: rubricas
description: Cria, lê e altera rubricas de avaliação no servidor Rubricas (rubrica.vini84200.cc) pela API em TOML. Use quando o usuário pedir para criar, montar, ajustar ou revisar uma rubrica, um critério ou um roteiro de correção.
---

# Rubricas

Cada rubrica é um arquivo TOML no servidor. Leia só a que vai mudar e grave o arquivo inteiro de volta.

```sh
A="Authorization: Bearer $RUBRICAS_TOKEN"
curl -sfH "$A" "$RUBRICAS_URL/api/rubricas.txt"                  # id | título | critérios | passos
curl -sfH "$A" "$RUBRICAS_URL/api/rubricas/ID.toml"              # o arquivo
curl -sH "$A" -X PUT --data-binary @arq.toml "$RUBRICAS_URL/api/rubricas/ID.toml"   # grava ou cria
```

O `PUT` valida antes de gravar. Ele responde com uma linha, ou com `{"erro": "linha N, coluna M: …"}`. Se faltar `RUBRICAS_URL` ou `RUBRICAS_TOKEN`, peça ao usuário: o token sai de `rubricas token <nome>` no servidor. O ID é o nome do arquivo e vira parte da URL, então use minúsculas e hífens.

## Formato

```toml
titulo = "Prática 5: listas"

[[criterios]]                       # passo de roteiro: orienta quem corrige, não vale nota
tipo = "roteiro"
nome = "Rode com entrada vazia"
descricao = "Execute `python main.py` e aperte Enter. Deve pedir de novo."

[[criterios]]
nome = "Trata entrada vazia"
escala = { tipo = "binaria", pontos = 1.0 }              # atende ou não

[[criterios]]
nome = "Modularização"
descricao = "Aparece no cartão durante a correção."
peso = 2.0                                               # opcional, multiplica os pontos
[criterios.escala]
tipo = "niveis"
[[criterios.escala.niveis]]
rotulo = "Excelente"
descritor = "Funções curtas e bem nomeadas."
pontos = 2.0
[[criterios.escala.niveis]]
rotulo = "Ausente"
pontos = 0.0

[[criterios]]
nome = "Legibilidade"
escala = { tipo = "livre", max = 2.0, passo = 0.25 }     # nota de 0 a max

[[criterios]]
nome = "Estado em structs"
escala = { tipo = "ponto_unico", esperado = "Estado em structs; salva em arquivo.", max = 2.0 }
```

- **A ordem importa:** cada passo de roteiro aparece na correção logo antes do critério que vem depois dele. Intercale cada passo com o critério que ele verifica.
- **Passos de roteiro** não têm `escala` nem `peso`. `descricao` aceita Markdown simples: `código`, blocos ``` ```, listas, **negrito** e *itálico*.
- **Identidade:** critérios e níveis sem `id` são ligados aos já gravados pelo nome. Para **renomear** sem perder as correções feitas, mantenha o id antigo acrescentando `id = "nome-antigo-em-slug"` (minúsculas, sem acento, com hífens) ao item renomeado.
- **Linhas `id =`:** só aparecem quando o id não sai do nome. Preserve as que vierem no arquivo.
- **Critério obrigatório:** a rubrica precisa de pelo menos um critério com escala. Uma escala `niveis` tem pelo menos dois níveis.
