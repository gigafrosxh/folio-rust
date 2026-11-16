//! Programming languages Folio highlights and runs (same table as the Swift app). The editor gets the
//! table once and highlights in the web view; runs use `run_plans`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub file_extension: &'static str,
    pub keywords: Vec<&'static str>,
    pub builtins: Vec<&'static str>,
    pub line_comments: Vec<&'static str>,
    pub block_comment: Option<[&'static str; 2]>,
    pub string_delimiters: Vec<&'static str>,
    pub case_insensitive: bool,
    pub capitalized_are_types: bool,
    pub attribute_prefix: Option<&'static str>,
    /// (regex, token kind)
    pub extra_rules: Vec<(&'static str, &'static str)>,
    /// Alternative ways to run, tried in order; placeholders `{file}`, `{out}`, `{dir}`.
    pub run_plans: Vec<Vec<Vec<&'static str>>>,
}

impl LanguageSpec {
    pub fn is_runnable(&self) -> bool {
        !self.run_plans.is_empty()
    }
}

fn w(s: &'static str) -> Vec<&'static str> {
    s.split(' ').filter(|x| !x.is_empty()).collect()
}

#[allow(clippy::too_many_arguments)]
fn spec(
    id: &'static str, name: &'static str, ext: &'static str, keywords: &'static str, builtins: &'static str,
    line: &[&'static str], block: Option<[&'static str; 2]>, strings: &[&'static str], attr: Option<&'static str>,
    plans: Vec<Vec<Vec<&'static str>>>,
) -> LanguageSpec {
    LanguageSpec {
        id,
        name,
        file_extension: ext,
        keywords: w(keywords),
        builtins: w(builtins),
        line_comments: line.to_vec(),
        block_comment: block,
        string_delimiters: strings.to_vec(),
        case_insensitive: false,
        capitalized_are_types: true,
        attribute_prefix: attr,
        extra_rules: vec![],
        run_plans: plans,
    }
}

const C: Option<[&str; 2]> = Some(["/*", "*/"]);

pub fn all() -> Vec<LanguageSpec> {
    let mut v = vec![
        spec("python", "Python", "py", "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case self",
            "print len range int str float list dict set tuple bool open enumerate zip map filter sorted sum min max abs isinstance type super object Exception",
            &["#"], None, &["\"\"\"", "'''", "\"", "'"], Some("@"), vec![vec![vec!["python3", "-u", "{file}"]], vec![vec!["python", "-u", "{file}"]]]),
        spec("javascript", "JavaScript", "js", "break case catch class const continue debugger default delete do else export extends finally for function if import in instanceof let new of return super switch this throw try typeof var void while with yield async await static get set null undefined true false",
            "console Math JSON Promise Array Object String Number Boolean Map Set Date Error RegExp window document",
            &["//"], C, &["`", "\"", "'"], None, vec![vec![vec!["node", "{file}"]], vec![vec!["bun", "run", "{file}"]], vec![vec!["deno", "run", "{file}"]]]),
        spec("typescript", "TypeScript", "ts", "break case catch class const continue debugger default delete do else enum export extends finally for function if implements import in instanceof interface let new of return super switch this throw try typeof var void while with yield async await static get set null undefined true false type namespace declare readonly private public protected abstract as keyof",
            "console Math JSON Promise Array Object String Number Boolean Map Set Date Error string number boolean any unknown never void",
            &["//"], C, &["`", "\"", "'"], Some("@"), vec![vec![vec!["bun", "run", "{file}"]], vec![vec!["deno", "run", "{file}"]], vec![vec!["tsx", "{file}"]], vec![vec!["node", "--experimental-strip-types", "{file}"]]]),
        spec("swift", "Swift", "swift", "associatedtype class deinit enum extension fileprivate func import init inout internal let open operator private protocol public rethrows static struct subscript typealias var break case continue default defer do else fallthrough for guard if in repeat return switch where while as catch false is nil super self Self throw throws true try async await some any actor nonisolated mutating override final lazy weak unowned convenience required",
            "print String Int Double Float Bool Array Dictionary Set Optional Character Data URL Date", &["//"], C, &["\"\"\"", "\""], Some("@"), vec![vec![vec!["swift", "{file}"]]]),
        spec("ruby", "Ruby", "rb", "BEGIN END alias and begin break case class def defined? do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require require_relative attr_accessor puts",
            "", &["#"], None, &["\"", "'"], Some("@"), vec![vec![vec!["ruby", "{file}"]]]),
        spec("bash", "Bash", "sh", "if then else elif fi case esac for select while until do done in function time return exit export local readonly declare echo printf cd source true false", "", &["#"], None, &["\"", "'"], Some("$"), vec![vec![vec!["bash", "{file}"]]]),
        spec("zsh", "Zsh", "zsh", "if then else elif fi case esac for select while until do done in function time return exit export local readonly declare typeset echo print printf cd source true false", "", &["#"], None, &["\"", "'"], Some("$"), vec![vec![vec!["zsh", "{file}"]]]),
        spec("fish", "Fish", "fish", "if else end for in while function return begin switch case and or not set echo printf string test true false", "", &["#"], None, &["\"", "'"], Some("$"), vec![vec![vec!["fish", "{file}"]]]),
        spec("c", "C", "c", "auto break case char const continue default do double else enum extern float for goto if inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void volatile while NULL true false bool",
            "printf scanf malloc free size_t uint8_t uint32_t int32_t int64_t uint64_t FILE", &["//"], C, &["\"", "'"], None,
            vec![vec![vec!["clang", "-std=c17", "-O1", "{file}", "-o", "{out}"], vec!["{out}"]], vec![vec!["gcc", "{file}", "-o", "{out}"], vec!["{out}"]]]),
        spec("cpp", "C++", "cpp", "alignas alignof and auto bool break case catch char class const constexpr const_cast continue decltype default delete do double dynamic_cast else enum explicit export extern false float for friend goto if inline int long mutable namespace new noexcept nullptr operator or private protected public return short signed sizeof static static_cast struct switch template this throw true try typedef typename union unsigned using virtual void volatile while override final",
            "std cout cin endl string vector map set unique_ptr shared_ptr size_t", &["//"], C, &["\"", "'"], None,
            vec![vec![vec!["clang++", "-std=c++20", "-O1", "{file}", "-o", "{out}"], vec!["{out}"]], vec![vec!["g++", "-std=c++20", "{file}", "-o", "{out}"], vec!["{out}"]]]),
        spec("go", "Go", "go", "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil iota",
            "fmt string int int64 float64 bool byte rune error make len cap append new panic", &["//"], C, &["`", "\"", "'"], None, vec![vec![vec!["go", "run", "{file}"]]]),
        spec("rust", "Rust", "rs", "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while",
            "println print format vec String Vec Option Some None Result Ok Err Box i32 i64 u8 u32 u64 usize f32 f64 bool str", &["//"], C, &["\""], None,
            vec![vec![vec!["rustc", "-O", "{file}", "-o", "{out}"], vec!["{out}"]]]),
        spec("java", "Java", "java", "abstract assert boolean break byte case catch char class const continue default do double else enum extends final finally float for goto if implements import instanceof int interface long native new package private protected public return short static strictfp super switch synchronized this throw throws transient try void volatile while var record true false null",
            "System String Integer List Map ArrayList HashMap Object", &["//"], C, &["\"", "'"], Some("@"), vec![vec![vec!["java", "{file}"]]]),
        spec("kotlin", "Kotlin", "kts", "as break class continue do else false for fun if in interface is null object package return super this throw true try typealias typeof val var when while by catch constructor data enum finally get import init internal lateinit open override private protected public sealed set suspend",
            "", &["//"], C, &["\"\"\"", "\"", "'"], Some("@"), vec![vec![vec!["kotlin", "{file}"]], vec![vec!["kotlinc", "-script", "{file}"]]]),
        spec("php", "PHP", "php", "abstract and array as break callable case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final finally fn for foreach function global goto if implements include instanceof interface isset list match namespace new or print private protected public require return static switch throw trait try unset use var while yield true false null",
            "", &["//", "#"], C, &["\"", "'"], Some("$"), vec![vec![vec!["php", "{file}"]]]),
        spec("perl", "Perl", "pl", "my our local sub if elsif else unless while until for foreach do last next redo return use package print say die eval", "", &["#"], None, &["\"", "'"], Some("$"), vec![vec![vec!["perl", "{file}"]]]),
        spec("lua", "Lua", "lua", "and break do else elseif end false for function goto if in local nil not or repeat return then true until while", "print pairs ipairs table string math require", &["--"], Some(["--[[", "]]"]), &["\"", "'"], None, vec![vec![vec!["lua", "{file}"]]]),
        spec("r", "R", "R", "if else repeat while function for in next break TRUE FALSE NULL Inf NaN NA library return", "", &["#"], None, &["\"", "'"], None, vec![vec![vec!["Rscript", "{file}"]]]),
        spec("sql", "SQL (SQLite)", "sql", "select from where insert into values update set delete create table drop alter add primary key foreign references index view join left right inner outer on group by order having limit offset as and or not null is in like between distinct union all case when then else end begin commit rollback integer text real blob default unique",
            "", &["--"], C, &["'", "\""], None, vec![vec![vec!["sqlite3", "-header", "-column", ":memory:", ".read {file}"]]]),
        spec("applescript", "AppleScript", "applescript", "tell end if then else repeat with times while return set to of on display dialog activate application get copy the my it as try error considering ignoring", "", &["--", "#"], Some(["(*", "*)"]), &["\""], None, vec![vec![vec!["osascript", "{file}"]]]),
        spec("html", "HTML", "html", "", "", &[], Some(["<!--", "-->"]), &["\"", "'"], None, vec![]),
        spec("css", "CSS", "css", "important media import from to", "", &[], C, &["\"", "'"], None, vec![]),
        spec("json", "JSON", "json", "true false null", "", &[], None, &["\""], None, vec![]),
        spec("yaml", "YAML", "yaml", "true false null yes no", "", &["#"], None, &["\"", "'"], None, vec![]),
        spec("markdown", "Markdown", "md", "", "", &[], None, &[], None, vec![]),
    ];
    for l in v.iter_mut() {
        match l.id {
            "bash" | "zsh" | "fish" | "perl" | "r" | "sql" | "applescript" | "html" | "css" | "json" | "yaml" | "markdown" => l.capitalized_are_types = false,
            _ => {}
        }
        match l.id {
            "sql" => l.case_insensitive = true,
            "c" | "cpp" => l.extra_rules = vec![(r"^\s*#\s*\w+.*$", "attribute")],
            "rust" => l.extra_rules = vec![(r"\b\w+!", "function")],
            "html" => l.extra_rules = vec![(r"</?[A-Za-z][\w:-]*|/?>", "keyword"), (r"\b[\w:-]+(?==)", "attribute"), (r"&\w+;", "number")],
            "css" => l.extra_rules = vec![(r"[\w-]+(?=\s*:[^:])", "attribute"), (r"#[0-9a-fA-F]{3,8}\b", "number")],
            "json" => l.extra_rules = vec![(r#""(?:\\.|[^"\\])*"(?=\s*:)"#, "attribute")],
            "yaml" => l.extra_rules = vec![(r"^\s*-?\s*[\w.-]+(?=\s*:)", "attribute")],
            "markdown" => l.extra_rules = vec![(r"^#{1,6} .*$", "keyword"), (r"\*\*[^*\n]+\*\*|__[^_\n]+__", "type"), (r"`[^`\n]+`", "string"), (r"\[[^\]\n]*\]\([^)\n]*\)", "function"), (r"^\s*([-*+]|\d+\.) ", "attribute")],
            _ => {}
        }
    }
    v
}

pub fn alias(id: &str) -> &str {
    match id {
        "py" => "python",
        "js" => "javascript",
        "ts" => "typescript",
        "sh" | "shell" => "bash",
        "rb" => "ruby",
        "c++" => "cpp",
        "golang" => "go",
        "rs" => "rust",
        "kt" => "kotlin",
        "md" => "markdown",
        "yml" => "yaml",
        "objc" => "c",
        "htm" => "html",
        other => other,
    }
}

pub fn spec_for(id: &str) -> Option<LanguageSpec> {
    let l = id.to_lowercase();
    let a = alias(&l).to_string();
    all().into_iter().find(|s| s.id == a)
}
