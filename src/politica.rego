# Quem pode fazer o quê. Avaliada pelo regorus (Rego, a linguagem do Open Policy Agent).
#
# Para adaptar: `rubricas politica > dados/politica.rego`, edite, reinicie o servidor.
# Um arquivo com erro impede o servidor de subir, em vez de liberar ou negar tudo às cegas.
#
# Entrada (`input`):
#   usuario.nome, usuario.papel   papel global: admin | professor | monitor | executor
#   recurso.tipo, recurso.id      tipo: sistema | avaliacao | rubrica | turma
#   acesso                        papel do usuário neste recurso (concedido a ele), ou null
#
# Saída: `acoes`, o conjunto de ações permitidas ao usuário sobre o recurso.
package rubricas

import rego.v1

# Papéis que podem ser concedidos em cada tipo de recurso, do menor para o maior.
# A tela de compartilhar mostra estes, nesta ordem.
papeis := {
	"avaliacao": ["leitor", "avaliador", "editor", "dono"],
	"rubrica": ["leitor", "editor", "dono"],
	"turma": ["leitor", "editor", "dono"],
}

papeis_globais := ["admin", "professor", "monitor", "executor"]

# Cada papel inclui as ações dos anteriores.
#   ver            abrir, listar, ler arquivos e relatórios
#   exportar       baixar as notas (CSV, planilha do Moodle)
#   corrigir       marcar a rubrica e editar o código do aluno
#   editar         mudar a avaliação, importar submissões, mudar as checagens
#   arquivar, apagar, compartilhar
#   ler_checagens  baixar pipeline e código para rodar as checagens (o executor)
acoes_do_papel := {
	"avaliacao": {
		"leitor": {"ver", "exportar", "ler_checagens"},
		"avaliador": {"ver", "exportar", "ler_checagens", "corrigir"},
		"editor": {"ver", "exportar", "ler_checagens", "corrigir", "editar", "arquivar"},
		"dono": {"ver", "exportar", "ler_checagens", "corrigir", "editar", "arquivar", "apagar", "compartilhar"},
	},
	"rubrica": {
		"leitor": {"ver"},
		"editor": {"ver", "editar", "arquivar"},
		"dono": {"ver", "editar", "arquivar", "apagar", "compartilhar"},
	},
	"turma": {
		"leitor": {"ver"},
		"editor": {"ver", "editar", "arquivar"},
		"dono": {"ver", "editar", "arquivar", "apagar", "compartilhar"},
	},
}

# Ações sobre o sistema como um todo.
#   criar           criar rubricas, turmas e avaliações (quem cria vira dono)
#   gerir_usuarios  criar, remover e mudar o papel de usuários
#   executar        pegar tarefas da fila de checagens e enviar resultados
#   ver_tudo        enxergar e mexer em tudo, mesmo sem acesso concedido
acoes_globais := {
	"admin": {"criar", "gerir_usuarios", "executar", "ver_tudo"},
	"professor": {"criar"},
	"monitor": set(),
	"executor": {"executar"},
}

default acoes := set()

acoes := acoes_globais[input.usuario.papel] if input.recurso.tipo == "sistema"

acoes := todas if {
	input.recurso.tipo != "sistema"
	input.usuario.papel == "admin"
}

acoes := concedidas | do_executor if {
	input.recurso.tipo != "sistema"
	input.usuario.papel != "admin"
}

todas := acoes_do_papel[input.recurso.tipo].dono

concedidas := acoes_do_papel[input.recurso.tipo][input.acesso] if input.acesso != null

default concedidas := set()

# O executor baixa o que precisa para rodar as checagens de qualquer avaliação,
# mas não vê notas nem correções.
do_executor := {"ler_checagens"} if {
	input.usuario.papel == "executor"
	input.recurso.tipo == "avaliacao"
}

default do_executor := set()
