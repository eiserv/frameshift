# Changelog

## 0.1.0 (2026-10-20)


### ⚠ BREAKING CHANGES

* **syntax:** `JsDialect` has a new variant, so exhaustive matches over it must handle `TypeScriptDeclaration`.
* **syntax:** parse TypeScript and TSX in an explicitly selected dialect
* **syntax:** parse_js_program and parse_js_expression take a JsDialect argument, Program nodes record it, and LexicalLimits gains max_jsx_text_bytes.

### Features

* add the read-once snapshot store and the JSON and JSONC parser ([#18](https://github.com/eiserv/frameshift/issues/18)) ([f84e662](https://github.com/eiserv/frameshift/commit/f84e662ae84358bdd2d915b0c61e6bcec7dd85dc))
* **bench:** generate a TypeScript, TSX, and declaration-file corpus ([3dcdc34](https://github.com/eiserv/frameshift/commit/3dcdc34abc0c4bce9d43b8aaf65d490d3d05eb85))
* **bench:** measure a generated JSX component corpus ([db7c6b5](https://github.com/eiserv/frameshift/commit/db7c6b55fa7d0026c039f9c1c1ed7d2727b5e274))
* **bench:** measure the source files of a directory ([1681333](https://github.com/eiserv/frameshift/commit/1681333a12f348c0ea125e034d4c5157e93be314))
* **bench:** validate generated corpus parser and rewriting contracts ([e4b6e6a](https://github.com/eiserv/frameshift/commit/e4b6e6a946656e442421ffc81dca0446215a56ec))
* **cli:** add frameshift inspect with the inspection document ([#25](https://github.com/eiserv/frameshift/issues/25)) ([ce34e53](https://github.com/eiserv/frameshift/commit/ce34e5333fa42bca6b192205cff7f1471de983de))
* **cli:** add help and truthful version output ([aae458c](https://github.com/eiserv/frameshift/commit/aae458c9648cf430dcd6b7cc7f29bbcf8f270878))
* **core:** add bounded findings and deterministic numbering ([#9](https://github.com/eiserv/frameshift/issues/9)) ([557e86c](https://github.com/eiserv/frameshift/commit/557e86c8ab1d8338703fda094d76bfe7c095b724))
* **core:** add bounded SHA-256 identities and streaming ([#11](https://github.com/eiserv/frameshift/issues/11)) ([32e0fff](https://github.com/eiserv/frameshift/commit/32e0fff9432ebe884073be48ba59fd2eec8fbb06))
* **core:** add immutable text and deterministic span rewriting ([#2](https://github.com/eiserv/frameshift/issues/2)) ([7d72cb9](https://github.com/eiserv/frameshift/commit/7d72cb9c9701a12f54ae1b8098d05ffb52a73d9b))
* **core:** add stable catalog and unit identifiers ([#8](https://github.com/eiserv/frameshift/issues/8)) ([7cb0a43](https://github.com/eiserv/frameshift/commit/7cb0a4357a4b4e80a0d0dfba5cbe56d09f3fc744))
* **core:** complete foundation collections and shared limits ([#12](https://github.com/eiserv/frameshift/issues/12)) ([b594bb3](https://github.com/eiserv/frameshift/commit/b594bb39722d6592e2a0f013e4ff2e1908eed970))
* **core:** validate canonical source and portable output paths ([#7](https://github.com/eiserv/frameshift/issues/7)) ([65fe785](https://github.com/eiserv/frameshift/commit/65fe7859823baac6ca4dc580ab619adb118dc6f7))
* **cputime:** add a Go helper that measures the processor time of a process tree ([fa679d7](https://github.com/eiserv/frameshift/commit/fa679d7d8480276a12e80f64ba95e11d2cfe3ca7))
* **discovery:** walk projects read-only with Git-compatible gitignore matching ([#13](https://github.com/eiserv/frameshift/issues/13)) ([79890a4](https://github.com/eiserv/frameshift/commit/79890a479d74fca20b516f62479b559b8ac3c2b8))
* **e2:** add the E2 driver with three fictional Next.js fixtures ([82a9db9](https://github.com/eiserv/frameshift/commit/82a9db920b4adeaa0dd7f0c68d2ff820b51aaa34))
* **emit:** add an Astro component printer with JSX conversion and provenance ([a5c9bf5](https://github.com/eiserv/frameshift/commit/a5c9bf588fe1fd5a767ba4e9562aa32b6e7ab75c))
* **fuzz:** add a coverage-guided fuzzer for parsers, scanners, and edits ([9977734](https://github.com/eiserv/frameshift/commit/9977734b1ce623bc539a07b2f832ebdc5da481c3))
* **package:** add YAML 1.2 parsing and pnpm 9.0 lockfiles ([#22](https://github.com/eiserv/frameshift/issues/22)) ([0fa41e5](https://github.com/eiserv/frameshift/commit/0fa41e5c44320f667ff5cc6baad0659a4b62e89f))
* **package:** detect workspaces and Next.js projects ([#24](https://github.com/eiserv/frameshift/issues/24)) ([94ec111](https://github.com/eiserv/frameshift/commit/94ec111b9c72add20058bcbf5741df89df4de6c7))
* **package:** model package.json, package managers, npm ranges, and npm and Bun lockfiles ([#19](https://github.com/eiserv/frameshift/issues/19)) ([5382718](https://github.com/eiserv/frameshift/commit/5382718d83621a6b9bb09d39d3a6101b2fae7be7))
* **package:** read Yarn classic and Berry lockfiles ([#23](https://github.com/eiserv/frameshift/issues/23)) ([cc0d0c3](https://github.com/eiserv/frameshift/commit/cc0d0c3fc12b2b6916992dfa2e130c24b5cd44da))
* **parser:** add bounded expression arena parser ([c22f2aa](https://github.com/eiserv/frameshift/commit/c22f2aae145d72d3bd629d55f25d7510572312f4))
* **parser:** add bounded JavaScript lexical primitives ([#4](https://github.com/eiserv/frameshift/issues/4)) ([e8252c4](https://github.com/eiserv/frameshift/commit/e8252c4551993e757f8fb175a27954c5036b6113))
* **parser:** add context-controlled template literal scanning ([b3ce408](https://github.com/eiserv/frameshift/commit/b3ce4081e740376be9d8b14d11137f47d5a1a1fe))
* **parser:** add numeric and context-controlled punctuation scanning ([6c656a1](https://github.com/eiserv/frameshift/commit/6c656a1d187bf05588a36c4fe145fd6158be92df))
* **parser:** classify identifier characters with pinned Unicode 18 tables ([1e425cf](https://github.com/eiserv/frameshift/commit/1e425cfb5f852275f5307dc738ef10b6af97c30d))
* **parser:** complete expression grammar with in-place pattern reinterpretation ([fecc28e](https://github.com/eiserv/frameshift/commit/fecc28e06296ebd299261814f7a62afef0316c2b))
* **parser:** decode the StringValue of string literal nodes on request ([ac66938](https://github.com/eiserv/frameshift/commit/ac66938ea763c6b76183ac2f309f2c5b8bb115e6))
* **parser:** parse complete scripts and modules with sloppy mode and early errors ([a945583](https://github.com/eiserv/frameshift/commit/a9455839352fd9aa674028760e0f9a9df58d56f3))
* **parser:** scan Annex B comments and legacy octal forms explicitly ([ad2744a](https://github.com/eiserv/frameshift/commit/ad2744a3dc15e0cc24d4eda7350b7ca7d0142cbd))
* **parser:** scan identifier names, private names, and reserved words ([6889d00](https://github.com/eiserv/frameshift/commit/6889d002b4b28f0fd58998120a43dbc87c5f683c))
* **parser:** validate regular-expression literals with Annex B ([e6af38c](https://github.com/eiserv/frameshift/commit/e6af38c12c9cacc5463b4c3bb4fcd4fd23287604))
* **syntax:** decode character references with pinned JSX and HTML tables ([beb2cdc](https://github.com/eiserv/frameshift/commit/beb2cdc6e842c2666aeaf026ec6596cfc87ff646))
* **syntax:** parse JSX in an explicitly selected dialect ([91fb5a9](https://github.com/eiserv/frameshift/commit/91fb5a97586441f5b617edffb39d1147982cec77))
* **syntax:** parse TypeScript and TSX in an explicitly selected dialect ([e44a376](https://github.com/eiserv/frameshift/commit/e44a3766210423f4b3601d3844d910f7f4d2bc82))
* **syntax:** parse TypeScript declaration files as an ambient dialect ([9dd3be7](https://github.com/eiserv/frameshift/commit/9dd3be7a51d2172bce531aea9934a35313c72b92))


### Bug Fixes

* **bench:** apply the E1 memory budget to whole-corpus peak demand ([d38f9a9](https://github.com/eiserv/frameshift/commit/d38f9a9ac244dded7ed74b63739e241068b22247))
* **bench:** name the token boundary contract in the check summary ([0cf33bd](https://github.com/eiserv/frameshift/commit/0cf33bd28c79e3ab8e6452db80cf31a62cda188a))
* **emit:** walk expression trees iteratively in the JSX conversion ([afe6fe9](https://github.com/eiserv/frameshift/commit/afe6fe9ce2a13fc766536638fc2565b6f74cb323))
* **parser:** end arrow return types with null, true, or false ([8c4b49f](https://github.com/eiserv/frameshift/commit/8c4b49f64c62e1159d044ab5c88ae153935d67a1))
* **parser:** match the pinned compiler on cases found by the TypeScript corpus ([914f58c](https://github.com/eiserv/frameshift/commit/914f58cc3846a7bdf823be6f99e5006ba8cd4dde))
* **parser:** run each TypeScript speculation once per position and state ([c42b749](https://github.com/eiserv/frameshift/commit/c42b7492c212d175c9ace50f4a3239526e0f569b))
* **parser:** span bodyless signatures from their first token and require function expression bodies ([91c084e](https://github.com/eiserv/frameshift/commit/91c084e10826f01add1147c19f19bf1e6e62a725))
* **parser:** start every TypeScript node at its first token ([473f74e](https://github.com/eiserv/frameshift/commit/473f74e6c6388d53f0eae4e769bf1291becf4c7d))
* **parser:** start type predicates at their first token ([6aa6baa](https://github.com/eiserv/frameshift/commit/6aa6baaa0024926ebb30498736602895fbde7b4e))
* **parser:** stop retrying failed TypeScript arrow function heads ([fa7fbae](https://github.com/eiserv/frameshift/commit/fa7fbae3aba358a0c663f8060682824260f2132c))
* **security:** clarify allocator safety and avoid fixture path logging ([#20](https://github.com/eiserv/frameshift/issues/20)) ([31f7126](https://github.com/eiserv/frameshift/commit/31f712629f207cebb7a19aae6c4b1d4b40655873))


### Performance Improvements

* **parser:** index punctuator candidates by first byte ([fc42d9e](https://github.com/eiserv/frameshift/commit/fc42d9ec89825f8a5928147207197f3ced085f86))
* **parser:** inline the check for already skipped trivia ([de93f22](https://github.com/eiserv/frameshift/commit/de93f22cbb7b4631a8ff17453d905a186d2fea8c))
* **parser:** reuse child-list buffers ([81ab243](https://github.com/eiserv/frameshift/commit/81ab24317d8d601b8eaf0b44c10e92c3a0fc4775))
* **parser:** reuse repeated lookahead at the same offset ([c45b492](https://github.com/eiserv/frameshift/commit/c45b492816a309b71aaf2f5e7243217b4048b29b))
* **parser:** scan trivia and ASCII names without decoding ([9c84df5](https://github.com/eiserv/frameshift/commit/9c84df50eb60767ff5272f5721547fe8405164f7))
