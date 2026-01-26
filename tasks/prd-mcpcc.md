# PRD (V1.0): `mcpcc` — C Derleme Sürücüsü + MCP Tool Şeması + Yerel MCP Server (Linux Mint)

**Hedef okur:** Ralph (tek seferde implement edecek)
**Platform:** Linux Mint (x86_64)
**Hedef MCP spesifikasyonu:** MCP Spec **2025-11-25** (Tools capability) ([Model Context Protocol][1])
**Taşıma (transport):** **stdio** (JSON-RPC 2.0, stdin/stdout) ([Model Context Protocol][2])
**LLM sağlayıcısı:** OpenRouter (OpenAI-uyumlu API, `Authorization: Bearer ...`) ([OpenRouter][3])

---

## 0) Tek cümle özet

`mcpcc`, gcc/clang derleme çağrısını **değiştirmeden** wrap eder; link aşamasında bir executable çıktığında aynı anda (1) o binary için **MCP tool tanımları** içeren `mcp.json` üretir, (2) bu `mcp.json`’ı servis eden ve tool çağrısı geldiğinde hedef binary’yi **spawn edip çalıştıran** bir **MCP server executable** üretir.

---

## 1) Kapsam ve Terminoloji

### 1.1 V1 kapsamı (in-scope)

* Tek komutla C kaynaklarının derlenmesi (passthrough wrapper).
* Link sonucu **executable** oluştuğunda:

  * `mcp.json` üretimi (en az 1 fallback tool + mümkünse structured tool).
  * “Generated MCP server” executable üretimi (generic server binary kopyalama yaklaşımı).
  * `argp` ve `getopt_long` pattern’larından parametre/option şeması çıkarımı.
  * Kaynaktan gelen doc string/comment/string literal’ları derleyip **LLM ile kısa açıklamalar üretme** (zorunlu, fail-fast default).
  * Deterministik bir “manifest” ve log/diagnostics.
* Basit/sağlam annotation mekanizması ile heuristics’i override edebilme.

### 1.2 V1 dışı (out-of-scope)

* “Tam C compiler” yazımı.
* Windows host desteği.
* GUI → MCP UI dönüşümü.
* Otomatik FFI (shared library) tabanlı tool üretimi.
* Güvenlik sandbox’ını zorunlu kılma (opsiyonel flag olarak “sonraki sürüm”).
* CLI’nın semantik niyetini %95 doğru anlama (hedef: **yapısal şema**).

### 1.3 Tanımlar

* **Wrapper**: `mcpcc`’nin clang/gcc argümanlarını geçirerek aynı çıktıyı üretmesi.
* **Structured tool**: seçenek/arg şemasını object properties olarak sunan tool.
* **Fallback tool**: `run_raw` gibi “argv’yi ham alan” tool.
* **Extractor**: argp/getopt/annotation kaynaklarını okuyup internal ToolSpec üreten modül.
* **MCP Tool**: MCP `tools` capability’sinde listelenen tool nesnesi. Tool nesnesi name/description/inputSchema (ve opsiyonel outputSchema) içerir. ([Model Context Protocol][1])

---

## 2) Ürün hedefleri

1. Derleme çıktısı **normal gcc/clang ile eşdeğer** olsun (aynı binary, aynı exit code).
2. Link sonucu executable üretiliyorsa **her zaman**:

   * `mcp.json` üretilsin.
   * `.mcp-server` üretilsin.
3. `argp` / `getopt_long` ile yazılmış CLI’larda, option’ların büyük kısmı otomatik şemaya yansısın (V1 hedefi: %80+ option coverage).
4. Açıklama alanları LLM ile kısa ve düzgün üretilecek (zorunlu).
5. Üretim süreci:

   * deterministik,
   * cache’li,
   * loglanabilir,
   * hata mesajları eyleme dönük.

---

## 3) Başarı metrikleri (V1)

* `argp` / `getopt_long` kullanan örneklerde:

  * option’ların **%80+**’i schema’da property olarak görünsün.
* `mcpcc` ek süresi:

  * küçük projelerde makul (hedef: +%20 altı), LLM çağrısı cache ile tekrar build’de ~0 ek maliyet.
* `mcp-server`:

  * aynı input → aynı argv sıralaması (deterministik),
  * stdout/stderr/exitCode alanları her çağrıda dönsün.

---

## 4) Karara bağlanan “Open Questions” (artık açık soru yok)

### Q1 — Varsayılan derleyici clang mı gcc mi?

**Karar:**

* **Derleme (passthrough)**: kullanıcı ne istiyorsa onu kullanır.

  * `--mcpcc-cc <path>` verilmişse onu kullan.
  * Yoksa `CC` env varsa onu kullan.
  * Yoksa `clang` varsa `clang`, yoksa `gcc`.
* **Analiz**: V1’de **libclang** ile AST tabanlı analiz hedeflenir. Libclang yoksa extractor’lar **best-effort text fallback** ile çalışır ve structured tool çıkaramazsa `run_raw` ile devam eder.

### Q2 — LLM çağrısı başarısız olursa fail-fast mı?

**Karar:**

* **Default:** fail-fast (derleme başarıyla bitse bile MCP artifact üretimi başarısız sayılır ve `mcpcc` non-zero döner).
* **Opsiyon:** `--mcpcc-llm-mode=best-effort` ile minimum deterministik açıklamalarla devam edebilir (manifestte işaretlenir).
  Bu opsiyon, CI/test ve offline senaryolar için gerekli.

### Q3 — Positional arg/subcommand şeması V1’de ne kadar ileri?

**Karar (V1):**

* Subcommand analizi **yok** (argv[1] karşılaştırmaları, dispatch tablosu vb. out-of-scope).
* Structured tool her zaman opsiyonlardan sonra eklenebilecek bir `args: string[]` property’si içerir (kalan positional’lar).
* Daha ileri positional semantik (ör. `INPUT`, `OUTPUT` isimlendirmesi) sadece:

  * `argp`’nin `args_doc`/doc string’inden **çok basit** şekilde,
  * veya annotation ile yapılır.

### Q4 — Varsayılan timeout/stdout limitleri?

**Karar (V1 varsayılanları):**

* `timeoutMs`: **30_000** (30s)
* `maxStdoutBytes`: **1_048_576** (1 MiB)
* `maxStderrBytes`: **1_048_576** (1 MiB)
* Sunucu bunları global default olarak uygular; tool bazında annotation ile override edilebilir.

### Q5 — Annotation DSL formatı?

**Karar (V1):** GCC/Clang uyumlu, derlemeyi bozmayacak **section’a gömülen JSON string** yaklaşımı.

* Kullanıcı C koduna `#include "mcpcc_annot.h"` ekler.
* Makrolar **string literal JSON** taşır ve `.mcpcc` section’ına gömülür.
* `mcpcc` analiz sırasında bu JSON’ları okuyup heuristics’i override eder.

---

## 5) Kullanıcı hikâyeleri (güncellenmiş)

### US-001 — Wrapper derleme (clang/gcc passthrough)

**Kabul kriterleri**

* `mcpcc` *tüm bilinmeyen argümanları* underlying derleyiciye aynen geçirir.
* Exit code:

  * Derleyici başarısızsa `mcpcc` aynı non-zero (tercihen aynı) exit code ile döner.
  * MCP üretimi bu durumda çalışmaz.
* `mcpcc --mcpcc-print-cc` ile seçilen underlying compiler path yazdırılabilir (debug).

### US-002 — Her durumda fallback tool

**Kabul kriterleri**

* Link sonucu executable üretilen her build’de `mcp.json` içinde **en az bir tool** vardır: `<bin>.run_raw`.
* Bu tool’un `inputSchema`’sı null değildir ve JSON Schema object’idir (MCP Tools kuralı). ([Model Context Protocol][1])

### US-003 — `argp` extractor

**Kabul kriterleri**

* `struct argp_option[]` initializer’larından:

  * long name, short key, arg gereksinimi, doc string çıkarılır.
* `OPTION_ARG_OPTIONAL` gibi flag’ler arg gereksinimine yansır.
* Extractor başarılıysa structured tool üretilir.

### US-004 — `getopt_long` extractor

**Kabul kriterleri**

* `struct option long_options[]` initializer’larından:

  * name, has_arg (no/required/optional) çıkarılır.
* `getopt_long(..., optstring, ...)` içindeki optstring parse edilerek short option’ların arg gereksinimi eşlenir.
* Extractor başarılıysa structured tool üretilir.

### US-005 — Annotation ile deterministik override

**Kabul kriterleri**

* Kullanıcı tool adı, açıklama, parametre mapping/description override edebilir.
* Merge önceliği deterministik: **annotation > argp/getopt > fallback**.

### US-006 — OpenRouter ile açıklama üretimi (zorunlu)

**Kabul kriterleri**

* Default modda LLM çağrısı yapılmadan schema finalize edilemez.
* Prompt’a tam kaynak kod değil, sadece “özet paket” gider.
* Cache: aynı özet paket + model + promptVersion için ikinci kez API çağrısı yapılmaz.

### US-007 — MCP server binary (stdio) + spawn

**Kabul kriterleri**

* `.<bin>.mcp-server` çalışınca MCP handshake’i yapar ve tools listeler. ([Model Context Protocol][2])
* Tool çağrısında executable spawn edilir; stdout/stderr/exitCode döner.

### US-008 — Manifest + çıktılar + DX

**Kabul kriterleri**

* Artifact path’leri deterministik.
* Manifest; compile args, extractor sonucu, llm modeli, cache hit/miss bilgilerini içerir.
* `--mcpcc-verbose` ile detaylı log; defaultta kısa log.

---

## 6) CLI Tasarımı (mcpcc)

### 6.1 Komut formu

Önerilen ana form (ambiguity yok):

```bash
mcpcc [mcpcc_flags...] -- [compiler_and_linker_flags...]
```

Kullanıcı ergonomisi için `--` olmadan da çalışır:

* `--mcpcc-` prefix’li olanlar wrapper flag kabul edilir
* diğerleri compiler flag kabul edilir

### 6.2 `mcpcc` flag’leri (V1)

* `--mcpcc-cc <path>`: underlying compiler (clang/gcc)
* `--mcpcc-artifacts-dir <dir>`: artifact’ların yazılacağı dizin (default: binary’nin bulunduğu dizin)
* `--mcpcc-mcp-json-out <path>`: mcp.json tam path override
* `--mcpcc-server-out <path>`: server binary tam path override
* `--mcpcc-manifest-out <path>`: manifest tam path override
* `--mcpcc-llm-model <string>`: OpenRouter model id (default: “küçük/ucuz bir model”; implementerde sabitlenebilir)
* `--mcpcc-llm-mode required|best-effort|off`:

  * default `required`
  * `off` sadece `MCPCC_ALLOW_NO_LLM=1` varsa izinli (test modu)
* `--mcpcc-cache-dir <dir>`: default `~/.cache/mcpcc`
* `--mcpcc-verbose`
* `--mcpcc-version`
* `--mcpcc-help`

### 6.3 Ortam değişkenleri (V1)

* `OPENROUTER_API_KEY` (zorunlu, required modda) ([OpenRouter][3])
* `MCPCC_CC`, `MCPCC_CACHE_DIR`, `MCPCC_LLM_MODEL`, `MCPCC_LLM_MODE`, `MCPCC_ARTIFACTS_DIR`

---

## 7) Artifact çıktıları ve isimlendirme

### 7.1 Çıktı üretim koşulu

* `mcpcc` **yalnızca link sonucu bir executable oluştuğunda** MCP artifact üretir.
* Eğer `-c` / `-E` / `-S` / `-shared` gibi modlar tespit edilirse:

  * sadece passthrough compile yapılır
  * MCP üretimi **yapılmaz** (manifestte “skipped: not an executable link” yazılabilir)

### 7.2 Varsayılan path kuralları

`bin_path` = compiler `-o` ile üretilen executable (yoksa `a.out`)

`dir` = `--mcpcc-artifacts-dir` yoksa `dirname(bin_path)`

`base` = `basename(bin_path)`

Üretilenler:

* Binary: `dir/base` (mevcut derleyici çıktısı)
* MCP tool bundle: `dir/base.mcp.json`
* MCP server: `dir/base.mcp-server`
* Manifest: `dir/base.mcpcc-manifest.json`
* LLM cache: `cache_dir/llm/<sha256>.json`

### 7.3 Atomik yazım

* `mcp.json` ve manifest:

  * önce temp dosyaya yazılır
  * JSON parse edilip doğrulanır
  * sonra atomik rename ile final path’e alınır

---

## 8) `mcp.json` formatı (kesin, implementasyon sözleşmesi)

### 8.1 Dosya yapısı

`mcp.json` bir “tool bundle” dosyasıdır:

```json
{
  "mcpccVersion": "1.0.0",
  "mcpSpecVersion": "2025-11-25",
  "binary": {
    "path": "./myprog",
    "defaultCwd": null
  },
  "tools": [
    {
      "name": "myprog",
      "title": "myprog",
      "description": "LLM-generated short description...",
      "inputSchema": { /* JSON Schema object (not null) */ },
      "outputSchema": { /* JSON Schema object (optional, recommended) */ },

      "x-mcpcc": {
        "kind": "structured",
        "argvMapping": {
          "options": [
            {
              "property": "verbose",
              "long": "--verbose",
              "short": "-v",
              "takesValue": false,
              "valueStyle": "separate",
              "repeatable": false,
              "position": 10
            }
          ],
          "positionalProperty": "args"
        },
        "exec": {
          "timeoutMs": 30000,
          "maxStdoutBytes": 1048576,
          "maxStderrBytes": 1048576
        }
      }
    },
    {
      "name": "myprog.run_raw",
      "title": "myprog.run_raw",
      "description": "Run with raw argv.",
      "inputSchema": { /* ... */ },
      "outputSchema": { /* ... */ },

      "x-mcpcc": {
        "kind": "raw"
      }
    }
  ]
}
```

### 8.2 MCP Tool alan kuralları

* `name`:

  * unique olmalı
  * **1–128** karakter
  * izinli karakter seti: `[a-zA-Z0-9._-]` ([Model Context Protocol][1])
* `inputSchema`:

  * **null olamaz** (MCP Tools kuralı). ([Model Context Protocol][1])
  * “parametresiz tool” için önerilen minimal schema: `type: object`, `properties: {}`, `additionalProperties: false`. ([Model Context Protocol][1])
* `outputSchema`:

  * V1’de önerilir (server `structuredContent` döndüreceği için)

---

## 9) Structured tool şema kuralları (V1)

### 9.1 Ortak property’ler

Structured tool inputSchema:

* `type: "object"`
* `additionalProperties: false`
* `properties`:

  * Extract edilen option/flag’ler
  * `args` (positional): `type: array`, `items: string`, default `[]`

### 9.2 Option → JSON Schema type mapping (deterministik)

Extractor, her option için:

* Arg yoksa (flag): `boolean`
* Arg varsa:

  * default: `string`
  * integer heuristics (sadece çok güvenli):

    * arg placeholder `N|NUM|NUMBER|COUNT|PORT` gibi regex → `integer`
  * float heuristics:

    * placeholder `RATE|FLOAT|SECONDS` gibi → `number`
  * enum heuristics (konservatif):

    * doc string içinde `one of: a, b, c` gibi net pattern → `enum: ["a","b","c"]`
  * aksi halde string

> Not: Tip tahmini agresif yapılmaz; yanlış tipten kaçınmak daha değerlidir. Semantik doğruluk hedefi yok.

### 9.3 Required/optional

* Çoğu CLI option default optional.
* “required” sadece annotation ile işaretlenebilir (V1).

### 9.4 Optional-argument seçenekleri

* `optional_argument` / argp optional arg:

  * schema: `string` (optional)
  * **Boş string** (`""`) verilirse “flag-only” olarak serialize edilir.
  * `null/omitted` → hiç eklenmez.

### 9.5 Argv sıralaması

Deterministik:

1. Option’lar **extractor keşif sırasına** göre (array initializer sırası).
2. Sonra `args[]` içindeki positional’lar, verilen sırayla eklenir.

---

## 10) Extractor Tasarımı (V1)

### 10.1 Extractor seçim sırası

1. Annotation extractor (varsa tool/param override’ları uygula)
2. `argp` extractor
3. `getopt_long` extractor
4. Fallback only (`run_raw`)

> Birden fazla kaynak varsa merge yapılır: annotation en üstte override eder.

### 10.2 Argp extractor (detay)

* Hedef: `struct argp_option <name>[] = { ... }` initializer’larını bulmak.
* Her entry için:

  * `name` → `--<name>` long flag
  * `key` → `-<char>` short flag (char literal ise)
  * `arg`:

    * `NULL/0` → takesValue false
    * string → takesValue true
  * `flags`:

    * `OPTION_ARG_OPTIONAL` var → optional-arg
  * `doc` string literal → ham help materyali (LLM özet paketine girer)

### 10.3 Getopt extractor (detay)

* `struct option long_options[]` initializer:

  * `name` → `--name`
  * `has_arg`:

    * `no_argument` → flag
    * `required_argument` → value required
    * `optional_argument` → optional-arg
  * `val` char literal ise short option eşleme adayı
* `optstring` parse:

  * `a:` required arg
  * `a::` optional arg
  * `a` no arg
* Help string:

  * V1: guaranteed değil. Sadece yakın comment/string literal toplanır (heuristic).

### 10.4 “Özet paket” üretimi (LLM’e giden minimal context)

Her tool için:

* toolName, binaryName
* param listesi:

  * long/short, takesValue, optionalArg, guessedType
  * ham doc/help string (varsa)
* maksimum X parametre (default 128)
* toplam metin boyutu sınırı (ör. 8 KB). Aşılırsa truncate + manifestte not düş.

---

## 11) Annotation DSL (V1, kesin)

### 11.1 `mcpcc_annot.h`

Proje, kullanıcıya dahil edilecek tek header sağlar. Önerilen API:

```c
// mcpcc_annot.h
#pragma once

#if defined(__GNUC__) || defined(__clang__)
  #define MCPCC_SECTION __attribute__((used, section(".mcpcc")))
#else
  #define MCPCC_SECTION
#endif

#define MCPCC_TOOL_JSON(json_literal) \
  static const char mcpcc_tool_##__COUNTER__[] MCPCC_SECTION = "MCPCC_TOOL:" json_literal;

#define MCPCC_PARAM_JSON(json_literal) \
  static const char mcpcc_param_##__COUNTER__[] MCPCC_SECTION = "MCPCC_PARAM:" json_literal;
```

Kullanım örneği:

```c
#include "mcpcc_annot.h"

MCPCC_TOOL_JSON("{\"name\":\"myprog\",\"title\":\"myprog\",\"description\":\"Override desc\"}");

MCPCC_PARAM_JSON("{\"tool\":\"myprog\",\"property\":\"verbose\",\"long\":\"--verbose\",\"short\":\"-v\",\"description\":\"More logs\",\"type\":\"boolean\"}");
```

### 11.2 Annotation JSON şemaları (V1)

**Tool annotation (`MCPCC_TOOL:`)**

* `name` (string, required)
* `title` (string, optional)
* `description` (string, optional)
* `timeoutMs` (int, optional)
* `maxStdoutBytes` (int, optional)
* `maxStderrBytes` (int, optional)

**Param annotation (`MCPCC_PARAM:`)**

* `tool` (string, required) → hangi tool’a ait
* `property` (string, required) → schema property adı
* `long` (string, optional) → örn. `--verbose`
* `short` (string, optional) → örn. `-v`
* `takesValue` (bool, optional)
* `type` (`"boolean"|"string"|"integer"|"number"`, optional)
* `repeatable` (bool, optional)
* `required` (bool, optional)
* `description` (string, optional)

### 11.3 Merge kuralları

* Tool düzeyi: annotation alanı verilmişse override eder; verilmemişse extractor’dan gelen korunur.
* Param düzeyi: aynı `property` için annotation override eder.
* Sıralama:

  * annotation param’ı için `position` verilmemişse, extractor sırası korunur.

---

## 12) OpenRouter LLM Entegrasyonu (V1)

### 12.1 Kimlik doğrulama

* `OPENROUTER_API_KEY` env’den okunur.
* HTTP header: `Authorization: Bearer <key>` ([OpenRouter][3])

### 12.2 Çağrı stratejisi (token-min)

* Tool başına **tek çağrı** (param listesiyle birlikte) → en düşük token.
* Prompt sabit “promptVersion” ile versiyonlanır; cache key’e dahil edilir.
* `temperature=0` (veya minimum) + kısa output kısıtları.

### 12.3 Beklenen çıktı formatı (strict JSON)

Modelden şu JSON beklenir:

```json
{
  "toolDescription": "string",
  "params": {
    "verbose": "string",
    "output": "string"
  }
}
```

* Sadece bilinen property’ler kabul edilir; fazlası ignore edilir.
* Boş/çok uzun açıklamalar:

  * minimum 5 karakter
  * maksimum 240 karakter (trim)

### 12.4 Cache

* Key = SHA-256( `promptVersion` + `model` + `analysis_summary_json` )
* `cache_dir/llm/<hash>.json` içerik: request + response + timestamp
* `--mcpcc-llm-mode=required` ise:

  * cache miss → API call zorunlu
  * API fail → build fail

---

## 13) MCP Server (generated executable) Tasarımı

### 13.1 Üretim yaklaşımı (V1)

* `mcpcc` dağıtımı içinde **generic** bir `mcpcc-mcp-server` binary bulunur.
* Build sonrası:

  * bu binary **kopyalanır** ve `base.mcp-server` adını alır.
* Server runtime’da `base.mcp.json` dosyasını okur.

> Bu yaklaşım “server üretimi” şartını sağlar ama her build’de Rust derlemesi yapmaz (hız + basitlik).

### 13.2 Protokol ve transport

* MCP server stdio transport kullanır. ([Model Context Protocol][2])
* MCP lifecycle: initialize → initialized → tools/list → tools/call. ([Model Context Protocol][1])
* Implementasyon kütüphanesi:

  * Rust için `rmcp` (MCP Rust SDK) tercih edilir. ([GitHub][4])

### 13.3 `tools/listTools`

* `mcp.json.tools[]` içindeki MCP Tool nesnelerini döndürür.

### 13.4 `tools/callTool` → spawn kuralları

* Tool bulunamazsa: isError=true, mesaj.
* Structured tool:

  * input object validate edilir (`additionalProperties:false`).
  * argv mapping `x-mcpcc.argvMapping` ile üretilir.
* Raw tool (`*.run_raw`):

  * `args[]` doğrudan argv’ye eklenir.

### 13.5 Çalıştırma ve çıktı yakalama

* `cwd`: default `binary.defaultCwd` veya server’ın current dir’i.
* `env`: default inherit; raw tool’da override edilebilir.
* stdin:

  * raw tool: opsiyonel `stdin` string gönderilirse pipe ile verilir.
  * structured tool: V1’de stdin yok (isterseniz ekleyin ama default minimal kalsın).

### 13.6 Timeout / output limit

* timeout aşılırsa:

  * process kill
  * `timedOut=true`
* stdout/stderr limit aşılırsa:

  * truncate
  * `truncatedStdout=true` / `truncatedStderr=true`

### 13.7 Tool result formatı

MCP tool result:

* `content`: kısa özet text (ör. `exitCode=0`)
* `structuredContent`:

  ```json
  {
    "stdout": "...",
    "stderr": "...",
    "exitCode": 0,
    "durationMs": 12,
    "timedOut": false,
    "truncatedStdout": false,
    "truncatedStderr": false
  }
  ```
* `isError`:

  * spawn error veya timeout → true
  * exitCode != 0 → true (V1 varsayılan)

> MCP Tools “tool result” ve structuredContent kavramları Tools spec’te tanımlıdır. ([Model Context Protocol][1])

---

## 14) Manifest (`*.mcpcc-manifest.json`) (kesin)

Örnek alanlar:

```json
{
  "mcpccVersion": "1.0.0",
  "timestamp": "2026-01-26T00:00:00Z",
  "host": { "os": "linux", "distro": "linuxmint" },
  "binary": { "path": "build/myprog" },

  "compiler": {
    "cc": "/usr/bin/clang",
    "argv": ["clang", "main.c", "-O2", "-o", "build/myprog"],
    "exitCode": 0
  },

  "analysis": {
    "usedLibclang": true,
    "extractors": ["annotation", "argp"],
    "structuredToolGenerated": true,
    "paramCount": 12,
    "notes": []
  },

  "llm": {
    "mode": "required",
    "provider": "openrouter",
    "model": "…",
    "cacheHit": true,
    "promptVersion": "v1"
  },

  "artifacts": {
    "mcpJson": "build/myprog.mcp.json",
    "server": "build/myprog.mcp-server"
  }
}
```

---

## 15) Hata yönetimi (mcpcc)

### 15.1 Exit code sözleşmesi (V1)

* Underlying compiler başarısız → compiler exit code propagate (mümkünse aynen).
* Wrapper kullanım hatası (arg parse) → 2
* Derleme OK ama MCP üretimi fail (LLM fail / JSON write fail / server copy fail) → 70

### 15.2 Loglama

* Default: kısa (1-2 satır).
* `--mcpcc-verbose`: extractor kararları, cache hit/miss, dosya path’leri.
* Loglar **stderr**’e.

---

## 16) Güvenlik notları (V1)

* Bu sistem tool çağrısında **target binary’yi çalıştırır**; bu, kullanıcı makinesinde kod çalıştırmaktır.
* V1’de sandbox zorunlu değildir; ancak:

  * server `--safe-mode` (sonraki sürüm) planlanabilir,
  * manifestte “unsafe by design” notu bulunabilir.

---

## 17) Test planı (Ralph için “tekte implement” checklist)

### 17.1 Golden örnekler

1. `argp` basit örnek

* `--verbose/-v`, `--output FILE`, positional `args[]`
  Beklenen: structured tool + run_raw.

2. `getopt_long` basit örnek

* long_options + optstring
  Beklenen: structured tool + run_raw.

3. No CLI parse (hiç argp/getopt yok)
   Beklenen: sadece run_raw.

4. Annotation override

* tool adı ve bir param açıklaması override
  Beklenen: override uygulanmış.

### 17.2 MCP server entegrasyon testi

* Server’ı stdio üzerinden başlat.
* `initialize` + `tools/listTools` doğrula.
* `tools/callTool` ile bir çağrı yap:

  * stdout/stderr/exitCode döndüğünü doğrula.

### 17.3 LLM testleri

* Cache hit senaryosu: aynı özet paket → ikinci build’de API çağrısı yok.
* `best-effort` mod: API key yokken “minimum description” ile devam.

---

## 18) Uygulama önerisi (teknik stack, V1)

* Dil: Rust (CLI + server)
* CLI arg parse: `clap`
* JSON: `serde`, `serde_json`
* Hash/cache: `sha2`
* HTTP: `reqwest`
* Log: `tracing`
* MCP server: `rmcp` (Rust SDK) ([GitHub][4])
* C analiz:

  * öncelik: `libclang` (clang-sys / clang crate)
  * fallback: kaynak metin tarama (sadece “pattern var mı” tespiti; structured tool garanti etmez)

---

## 19) Net deliverables (V1)

1. `mcpcc` executable
2. `mcpcc_annot.h` (annotation header)
3. `mcpcc-mcp-server` generic server executable (pakete gömülü)
4. Dokümantasyon:

   * `README`: kullanım + örnekler
   * `SPEC_mcp.json.md`: `mcp.json` formatı (bu PRD’nin 8. bölümü)
5. Örnek C projeleri + test suite (CI’de çalışacak)

