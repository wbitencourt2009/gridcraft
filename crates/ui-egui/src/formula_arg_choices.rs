//! Excel-style argument value lists: while the caret is in an argument that takes one of a fixed
//! set of values (XLOOKUP's match mode, VLOOKUP's range lookup, ...), the editor offers them.
//! Values use pt-BR syntax (`VERDADEIRO`/`FALSO`); each entry is (value, label, tip).

type Choice = (&'static str, &'static str, &'static str);

const RANGE_LOOKUP: &[Choice] = &[
    ("VERDADEIRO", "Correspondência aproximada", "Localiza o maior valor menor ou igual ao procurado; a primeira coluna deve estar em ordem crescente."),
    ("FALSO", "Correspondência exata", "Procura uma correspondência exata; se não for encontrada, retorna #N/D."),
];

const MATCH_TYPE: &[Choice] = &[
    ("1", "Menor que", "Localiza o maior valor menor ou igual ao procurado; a matriz deve estar em ordem crescente."),
    ("0", "Correspondência exata", "Procura uma correspondência exata; se não for encontrada, retorna #N/D."),
    ("-1", "Maior que", "Localiza o menor valor maior ou igual ao procurado; a matriz deve estar em ordem decrescente."),
];

const MATCH_MODE: &[Choice] = &[
    ("0", "Correspondência exata", "Procura uma correspondência exata; se não for encontrada, retorna #N/D."),
    ("-1", "Correspondência exata ou próximo item menor", "Se não houver correspondência exata, retorna o próximo item menor."),
    ("1", "Correspondência exata ou próximo item maior", "Se não houver correspondência exata, retorna o próximo item maior."),
    ("2", "Correspondência de caractere curinga", "Os caracteres *, ? e ~ têm significado especial."),
    ("3", "Correspondência regex", "O valor procurado é uma expressão regular."),
];

const SEARCH_MODE: &[Choice] = &[
    ("1", "Pesquisar do primeiro ao último", "Pesquisa a partir do primeiro item."),
    ("-1", "Pesquisar do último ao primeiro", "Pesquisa de trás para frente, a partir do último item."),
    ("2", "Pesquisa binária (classificação crescente)", "Pesquisa binária; a matriz deve estar em ordem crescente."),
    ("-2", "Pesquisa binária (classificação decrescente)", "Pesquisa binária; a matriz deve estar em ordem decrescente."),
];

const SUBTOTAL_FUNCTIONS: &[Choice] = &[
    ("1", "MÉDIA", "Média, incluindo linhas ocultas manualmente."),
    ("2", "CONT.NÚM", "Conta os números, incluindo linhas ocultas manualmente."),
    ("3", "CONT.VALORES", "Conta os valores não vazios, incluindo linhas ocultas manualmente."),
    ("4", "MÁXIMO", "Maior valor, incluindo linhas ocultas manualmente."),
    ("5", "MÍNIMO", "Menor valor, incluindo linhas ocultas manualmente."),
    ("6", "MULT", "Produto, incluindo linhas ocultas manualmente."),
    ("7", "DESVPAD", "Desvio padrão amostral, incluindo linhas ocultas manualmente."),
    ("8", "DESVPADP", "Desvio padrão populacional, incluindo linhas ocultas manualmente."),
    ("9", "SOMA", "Soma, incluindo linhas ocultas manualmente."),
    ("10", "VAR", "Variância amostral, incluindo linhas ocultas manualmente."),
    ("11", "VARP", "Variância populacional, incluindo linhas ocultas manualmente."),
    ("101", "MÉDIA", "Média, ignorando linhas ocultas."),
    ("102", "CONT.NÚM", "Conta os números, ignorando linhas ocultas."),
    ("103", "CONT.VALORES", "Conta os valores não vazios, ignorando linhas ocultas."),
    ("104", "MÁXIMO", "Maior valor, ignorando linhas ocultas."),
    ("105", "MÍNIMO", "Menor valor, ignorando linhas ocultas."),
    ("106", "MULT", "Produto, ignorando linhas ocultas."),
    ("107", "DESVPAD", "Desvio padrão amostral, ignorando linhas ocultas."),
    ("108", "DESVPADP", "Desvio padrão populacional, ignorando linhas ocultas."),
    ("109", "SOMA", "Soma, ignorando linhas ocultas."),
    ("110", "VAR", "Variância amostral, ignorando linhas ocultas."),
    ("111", "VARP", "Variância populacional, ignorando linhas ocultas."),
];

const AGGREGATE_FUNCTIONS: &[Choice] = &[
    ("1", "MÉDIA", "Média aritmética."),
    ("2", "CONT.NÚM", "Conta os números."),
    ("3", "CONT.VALORES", "Conta os valores não vazios."),
    ("4", "MÁXIMO", "Maior valor."),
    ("5", "MÍNIMO", "Menor valor."),
    ("6", "MULT", "Produto."),
    ("7", "DESVPAD.A", "Desvio padrão amostral."),
    ("8", "DESVPAD.P", "Desvio padrão populacional."),
    ("9", "SOMA", "Soma."),
    ("10", "VAR.A", "Variância amostral."),
    ("11", "VAR.P", "Variância populacional."),
    ("12", "MED", "Mediana."),
    ("13", "MODO.ÚNICO", "Valor mais frequente."),
    ("14", "MAIOR", "k-ésimo maior valor."),
    ("15", "MENOR", "k-ésimo menor valor."),
    ("16", "PERCENTIL.INC", "k-ésimo percentil, inclusivo."),
    ("17", "QUARTIL.INC", "Quartil, inclusivo."),
    ("18", "PERCENTIL.EXC", "k-ésimo percentil, exclusivo."),
    ("19", "QUARTIL.EXC", "Quartil, exclusivo."),
];

const AGGREGATE_OPTIONS: &[Choice] = &[
    ("0", "Ignorar SUBTOTAL e AGREGAR aninhados", "Ignora as funções SUBTOTAL e AGREGAR aninhadas."),
    ("1", "Ignorar linhas ocultas, SUBTOTAL e AGREGAR", "Ignora linhas ocultas e as funções SUBTOTAL e AGREGAR aninhadas."),
    ("2", "Ignorar valores de erro, SUBTOTAL e AGREGAR", "Ignora valores de erro e as funções SUBTOTAL e AGREGAR aninhadas."),
    ("3", "Ignorar linhas ocultas, erros, SUBTOTAL e AGREGAR", "Ignora linhas ocultas, valores de erro e as funções SUBTOTAL e AGREGAR aninhadas."),
    ("4", "Não ignorar nada", "Não ignora nada."),
    ("5", "Ignorar linhas ocultas", "Ignora as linhas ocultas."),
    ("6", "Ignorar valores de erro", "Ignora os valores de erro."),
    ("7", "Ignorar linhas ocultas e valores de erro", "Ignora linhas ocultas e valores de erro."),
];

const WEEKDAY_TYPE: &[Choice] = &[
    ("1", "Números 1 (domingo) a 7 (sábado)", "A semana começa no domingo."),
    ("2", "Números 1 (segunda-feira) a 7 (domingo)", "A semana começa na segunda-feira."),
    ("3", "Números 0 (segunda-feira) a 6 (domingo)", "A semana começa na segunda-feira, contando a partir de 0."),
    ("11", "Números 1 (segunda-feira) a 7 (domingo)", "A semana começa na segunda-feira."),
    ("12", "Números 1 (terça-feira) a 7 (segunda-feira)", "A semana começa na terça-feira."),
    ("13", "Números 1 (quarta-feira) a 7 (terça-feira)", "A semana começa na quarta-feira."),
    ("14", "Números 1 (quinta-feira) a 7 (quarta-feira)", "A semana começa na quinta-feira."),
    ("15", "Números 1 (sexta-feira) a 7 (quinta-feira)", "A semana começa na sexta-feira."),
    ("16", "Números 1 (sábado) a 7 (sexta-feira)", "A semana começa no sábado."),
    ("17", "Números 1 (domingo) a 7 (sábado)", "A semana começa no domingo."),
];

const WEEKNUM_TYPE: &[Choice] = &[
    ("1", "Domingo", "A semana começa no domingo."),
    ("2", "Segunda-feira", "A semana começa na segunda-feira."),
    ("11", "Segunda-feira", "A semana começa na segunda-feira."),
    ("12", "Terça-feira", "A semana começa na terça-feira."),
    ("13", "Quarta-feira", "A semana começa na quarta-feira."),
    ("14", "Quinta-feira", "A semana começa na quinta-feira."),
    ("15", "Sexta-feira", "A semana começa na sexta-feira."),
    ("16", "Sábado", "A semana começa no sábado."),
    ("17", "Domingo", "A semana começa no domingo."),
    ("21", "Segunda-feira (ISO 8601)", "Sistema ISO: a semana 1 contém a primeira quinta-feira do ano."),
];

const WEEKEND: &[Choice] = &[
    ("1", "Sábado, domingo", "Fim de semana no sábado e no domingo."),
    ("2", "Domingo, segunda-feira", "Fim de semana no domingo e na segunda-feira."),
    ("3", "Segunda-feira, terça-feira", "Fim de semana na segunda e na terça-feira."),
    ("4", "Terça-feira, quarta-feira", "Fim de semana na terça e na quarta-feira."),
    ("5", "Quarta-feira, quinta-feira", "Fim de semana na quarta e na quinta-feira."),
    ("6", "Quinta-feira, sexta-feira", "Fim de semana na quinta e na sexta-feira."),
    ("7", "Sexta-feira, sábado", "Fim de semana na sexta-feira e no sábado."),
    ("11", "Somente domingo", "Fim de semana só no domingo."),
    ("12", "Somente segunda-feira", "Fim de semana só na segunda-feira."),
    ("13", "Somente terça-feira", "Fim de semana só na terça-feira."),
    ("14", "Somente quarta-feira", "Fim de semana só na quarta-feira."),
    ("15", "Somente quinta-feira", "Fim de semana só na quinta-feira."),
    ("16", "Somente sexta-feira", "Fim de semana só na sexta-feira."),
    ("17", "Somente sábado", "Fim de semana só no sábado."),
];

const BASIS: &[Choice] = &[
    ("0", "EUA (NASD) 30/360", "Mês de 30 dias e ano de 360 dias, método americano."),
    ("1", "Real/real", "Dias reais do período sobre dias reais do ano."),
    ("2", "Real/360", "Dias reais do período sobre um ano de 360 dias."),
    ("3", "Real/365", "Dias reais do período sobre um ano de 365 dias."),
    ("4", "Europeu 30/360", "Mês de 30 dias e ano de 360 dias, método europeu."),
];

const DAYS360_METHOD: &[Choice] = &[
    ("FALSO", "Método dos EUA (NASD)", "Usa o método americano para datas no fim do mês."),
    ("VERDADEIRO", "Método europeu", "Datas no dia 31 passam a ser dia 30 do mesmo mês."),
];

const PAYMENT_TYPE: &[Choice] = &[
    ("0", "No fim do período", "Os pagamentos vencem no fim de cada período."),
    ("1", "No início do período", "Os pagamentos vencem no início de cada período."),
];

const SORT_ORDER: &[Choice] = &[
    ("1", "Crescente", "Classifica em ordem crescente."),
    ("-1", "Decrescente", "Classifica em ordem decrescente."),
];

const SORT_BY_COL: &[Choice] = &[
    ("FALSO", "Classificar por linha", "Classifica as linhas da matriz."),
    ("VERDADEIRO", "Classificar por coluna", "Classifica as colunas da matriz."),
];

const UNIQUE_BY_COL: &[Choice] = &[
    ("FALSO", "Comparar linhas", "Retorna as linhas distintas."),
    ("VERDADEIRO", "Comparar colunas", "Retorna as colunas distintas."),
];

const UNIQUE_ONCE: &[Choice] = &[
    ("FALSO", "Retornar todos os itens distintos", "Retorna cada item distinto uma vez."),
    ("VERDADEIRO", "Retornar itens que aparecem uma única vez", "Retorna só os itens que aparecem exatamente uma vez."),
];

const RANK_ORDER: &[Choice] = &[
    ("0", "Decrescente", "O maior número recebe a posição 1."),
    ("1", "Crescente", "O menor número recebe a posição 1."),
];

const CUMULATIVE: &[Choice] = &[
    ("VERDADEIRO", "Função de distribuição cumulativa", "Retorna a probabilidade acumulada."),
    ("FALSO", "Função de densidade de probabilidade", "Retorna a densidade (ou probabilidade pontual)."),
];

const TAILS: &[Choice] = &[
    ("1", "Unicaudal", "Distribuição unicaudal."),
    ("2", "Bicaudal", "Distribuição bicaudal."),
];

const TTEST_TYPE: &[Choice] = &[
    ("1", "Emparelhado", "Teste t emparelhado."),
    ("2", "Duas amostras com variância igual", "Duas amostras com variâncias iguais (homoscedástico)."),
    ("3", "Duas amostras com variância desigual", "Duas amostras com variâncias diferentes (heteroscedástico)."),
];

const QUARTILE_INC: &[Choice] = &[
    ("0", "Valor mínimo", "Retorna o menor valor."),
    ("1", "Primeiro quartil (25º percentil)", "Retorna o primeiro quartil."),
    ("2", "Valor mediano (50º percentil)", "Retorna a mediana."),
    ("3", "Terceiro quartil (75º percentil)", "Retorna o terceiro quartil."),
    ("4", "Valor máximo", "Retorna o maior valor."),
];

const QUARTILE_EXC: &[Choice] = &[
    ("1", "Primeiro quartil (25º percentil)", "Retorna o primeiro quartil."),
    ("2", "Valor mediano (50º percentil)", "Retorna a mediana."),
    ("3", "Terceiro quartil (75º percentil)", "Retorna o terceiro quartil."),
];

const DATEDIF_UNIT: &[Choice] = &[
    ("\"Y\"", "Anos completos", "Número de anos completos no período."),
    ("\"M\"", "Meses completos", "Número de meses completos no período."),
    ("\"D\"", "Dias", "Número de dias no período."),
    ("\"MD\"", "Dias, ignorando meses e anos", "Diferença entre os dias, ignorando meses e anos."),
    ("\"YM\"", "Meses, ignorando anos", "Diferença entre os meses, ignorando dias e anos."),
    ("\"YD\"", "Dias, ignorando anos", "Diferença entre os dias, ignorando anos."),
];

const TO_IGNORE: &[Choice] = &[
    ("0", "Manter todos os valores", "Não ignora nada."),
    ("1", "Ignorar vazios", "Ignora as células vazias."),
    ("2", "Ignorar erros", "Ignora os valores de erro."),
    ("3", "Ignorar vazios e erros", "Ignora células vazias e valores de erro."),
];

const SCAN_BY_COLUMN: &[Choice] = &[
    ("FALSO", "Examinar por linha", "Lê a matriz linha por linha."),
    ("VERDADEIRO", "Examinar por coluna", "Lê a matriz coluna por coluna."),
];

const IGNORE_EMPTY: &[Choice] = &[
    ("VERDADEIRO", "Ignorar células vazias", "Itens vazios não entram no resultado."),
    ("FALSO", "Incluir células vazias", "Itens vazios entram no resultado, gerando delimitadores seguidos."),
];

const ADDRESS_ABS: &[Choice] = &[
    ("1", "Absoluto", "Linha e coluna absolutas ($A$1)."),
    ("2", "Linha absoluta; coluna relativa", "A$1"),
    ("3", "Linha relativa; coluna absoluta", "$A1"),
    ("4", "Relativo", "Linha e coluna relativas (A1)."),
];

const A1_STYLE: &[Choice] = &[
    ("VERDADEIRO", "Estilo A1", "Referências no estilo A1."),
    ("FALSO", "Estilo L1C1", "Referências no estilo L1C1."),
];

/// The value list for argument `arg` (0-based) of the canonical function `name`.
pub(crate) fn choices(name: &str, arg: usize) -> &'static [Choice] {
    match (name, arg) {
        ("VLOOKUP" | "HLOOKUP", 3) => RANGE_LOOKUP,
        ("MATCH", 2) => MATCH_TYPE,
        ("XLOOKUP", 4) | ("XMATCH", 2) => MATCH_MODE,
        ("XLOOKUP", 5) | ("XMATCH", 3) => SEARCH_MODE,
        ("SUBTOTAL", 0) => SUBTOTAL_FUNCTIONS,
        ("AGGREGATE", 0) => AGGREGATE_FUNCTIONS,
        ("AGGREGATE", 1) => AGGREGATE_OPTIONS,
        ("WEEKDAY", 1) => WEEKDAY_TYPE,
        ("WEEKNUM", 1) => WEEKNUM_TYPE,
        ("NETWORKDAYS.INTL" | "WORKDAY.INTL", 2) => WEEKEND,
        ("YEARFRAC", 2) => BASIS,
        ("DAYS360", 2) => DAYS360_METHOD,
        ("PMT" | "PV" | "FV" | "NPER", 4) | ("RATE", 4) | ("IPMT" | "PPMT", 5) => PAYMENT_TYPE,
        ("SORT", 2) => SORT_ORDER,
        ("SORT", 3) => SORT_BY_COL,
        ("SORTBY", a) if a >= 2 && a % 2 == 0 => SORT_ORDER,
        ("UNIQUE", 1) => UNIQUE_BY_COL,
        ("UNIQUE", 2) => UNIQUE_ONCE,
        ("RANK" | "RANK.EQ" | "RANK.AVG", 2) => RANK_ORDER,
        ("NORM.S.DIST", 1)
        | ("EXPON.DIST" | "POISSON.DIST" | "T.DIST" | "CHISQ.DIST", 2)
        | ("NORM.DIST" | "BINOM.DIST" | "LOGNORM.DIST" | "GAMMA.DIST" | "WEIBULL.DIST" | "F.DIST" | "BETA.DIST" | "NEGBINOM.DIST", 3)
        | ("HYPGEOM.DIST", 4) => CUMULATIVE,
        ("T.TEST" | "TTEST", 2) | ("TDIST", 2) => TAILS,
        ("T.TEST" | "TTEST", 3) => TTEST_TYPE,
        ("QUARTILE" | "QUARTILE.INC", 1) => QUARTILE_INC,
        ("QUARTILE.EXC", 1) => QUARTILE_EXC,
        ("DATEDIF", 2) => DATEDIF_UNIT,
        ("TOCOL" | "TOROW", 1) => TO_IGNORE,
        ("TOCOL" | "TOROW", 2) => SCAN_BY_COLUMN,
        ("TEXTJOIN", 1) => IGNORE_EMPTY,
        ("ADDRESS", 2) => ADDRESS_ABS,
        ("ADDRESS", 3) | ("INDIRECT", 1) => A1_STYLE,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlookup_offers_its_match_modes() {
        let values: Vec<&str> = choices("XLOOKUP", 4).iter().map(|c| c.0).collect();
        assert_eq!(values, ["0", "-1", "1", "2", "3"]);
        assert!(choices("XLOOKUP", 0).is_empty());
        assert_eq!(choices("VLOOKUP", 3)[1].0, "FALSO");
    }
}
