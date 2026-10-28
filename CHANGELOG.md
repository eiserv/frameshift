# Changelog

## [0.2.0](https://github.com/eiserv/frameshift/compare/v0.1.0...v0.2.0) (2026-10-28)


### ⚠ BREAKING CHANGES

* **analyzer:** callers using explicit `Limits` struct literals must provide the new `analysis` resource limits. No planning, migration execution, or behavioral-equivalence claim is added.
* **features:** close M4 and detect V1 feature catalog occurrences ([#59](https://github.com/eiserv/frameshift/issues/59))
* **nextjs:** extract configuration, convention, and API facts ([#55](https://github.com/eiserv/frameshift/issues/55))

### Features

* **analyzer:** complete M5 analysis and reporting ([74d41b1](https://github.com/eiserv/frameshift/commit/74d41b1cca545a6ee03810817cdabb273f70d528))
* **features:** close M4 and detect V1 feature catalog occurrences ([#59](https://github.com/eiserv/frameshift/issues/59)) ([51084ac](https://github.com/eiserv/frameshift/commit/51084ac0a28455efa39af951ee6e27657f852a3b))
* **model:** assemble source project facts and route dependencies ([#53](https://github.com/eiserv/frameshift/issues/53)) ([c30110c](https://github.com/eiserv/frameshift/commit/c30110c454591401d4294041d5bc8629768a864f))
* **model:** resolve JSX render sites to component definitions ([#54](https://github.com/eiserv/frameshift/issues/54)) ([d5162fc](https://github.com/eiserv/frameshift/commit/d5162fcdedc861e35bf8098a0bb60d9d66cc5e6a))
* **modules:** resolve configuration inheritance from linked workspace packages ([#57](https://github.com/eiserv/frameshift/issues/57)) ([18eba93](https://github.com/eiserv/frameshift/commit/18eba93b72ba3052c45764fa541f9ec7bed98483))
* **modules:** resolve ordered module filename suffixes ([#52](https://github.com/eiserv/frameshift/issues/52)) ([06de5a7](https://github.com/eiserv/frameshift/commit/06de5a7f452b99d3877b91996a7918b8673f645b))
* **nextjs:** build the App Router route tree and render contexts ([#46](https://github.com/eiserv/frameshift/issues/46)) ([cbbcdfe](https://github.com/eiserv/frameshift/commit/cbbcdfe27a4add826b966b92e38e074e9630950a))
* **nextjs:** extract configuration, convention, and API facts ([#55](https://github.com/eiserv/frameshift/issues/55)) ([95caf82](https://github.com/eiserv/frameshift/commit/95caf8227970be822256d9d312681271c692bc79))
* **semantics:** analyze scopes, bindings, references, and exports ([#45](https://github.com/eiserv/frameshift/issues/45)) ([234d49a](https://github.com/eiserv/frameshift/commit/234d49ab72beaadb64e68ccc01745672f9390be9))
* **semantics:** evaluate configuration values and measure E3 coverage ([#48](https://github.com/eiserv/frameshift/issues/48)) ([bded55e](https://github.com/eiserv/frameshift/commit/bded55e81f5f0556b347f448f3ed2ea7fc610c90))
* **semantics:** resolve local modules and build a deterministic import graph ([#43](https://github.com/eiserv/frameshift/issues/43)) ([b67810c](https://github.com/eiserv/frameshift/commit/b67810ca453496e348f9d28b9b8c418fde6602ef))
* **semantics:** resolve local modules with an explicit Node10 profile ([#51](https://github.com/eiserv/frameshift/issues/51)) ([60edd25](https://github.com/eiserv/frameshift/commit/60edd258b2419bc8b8134e7fe649f382b16ce80b))
* **semantics:** resolve package imports, self-references, and directory entry points ([#44](https://github.com/eiserv/frameshift/issues/44)) ([b6308e5](https://github.com/eiserv/frameshift/commit/b6308e5f6e7203ec52d3630ee7c73507998f6a91))
* **syntax:** add the CSS tokenizer from the pinned CSS Syntax draft ([#28](https://github.com/eiserv/frameshift/issues/28)) ([d9a4f31](https://github.com/eiserv/frameshift/commit/d9a4f3146d159ecf81f0b7406ff4d5b541bd3a20))
* **syntax:** begin SCSS and indented Sass parsing ([#30](https://github.com/eiserv/frameshift/issues/30)) ([3139e3f](https://github.com/eiserv/frameshift/commit/3139e3f74db909a78e8578ea84f7ec0f1713f3cb))
* **syntax:** complete Sass parsing conformance against the normal reference ([#42](https://github.com/eiserv/frameshift/issues/42)) ([5b52066](https://github.com/eiserv/frameshift/commit/5b52066fcd47f53a2d0c28f4f4d1c7d727312ce4))
* **syntax:** extend Sass grammar and JavaScript diagnostic recovery ([#40](https://github.com/eiserv/frameshift/issues/40)) ([d056601](https://github.com/eiserv/frameshift/commit/d0566013d2599c99ccc78f3c7e7bc1bdf86a27d7))
* **syntax:** parse generic CSS structure with bounded recovery ([#29](https://github.com/eiserv/frameshift/issues/29)) ([93dab86](https://github.com/eiserv/frameshift/commit/93dab86c6840c9f04afd5c664d20e5dc7a640ee4))
* **syntax:** parse legacy indented Sass mixin shorthand ([#31](https://github.com/eiserv/frameshift/issues/31)) ([6030ad0](https://github.com/eiserv/frameshift/commit/6030ad02bd3be692c294b3b53979ffb85f475022))
* **syntax:** parse modern Sass if and recover JavaScript diagnostics ([#37](https://github.com/eiserv/frameshift/issues/37)) ([0914f99](https://github.com/eiserv/frameshift/commit/0914f99255e031497975cec305d79a88260601b1))
* **syntax:** parse plain CSS functions in Sass ([#38](https://github.com/eiserv/frameshift/issues/38)) ([e902689](https://github.com/eiserv/frameshift/commit/e90268912a3bce47f66976fa77ae81dff8922a61))
* **syntax:** parse Sass selectors with Dart Sass's evaluation grammars ([#32](https://github.com/eiserv/frameshift/issues/32)) ([f25fbc3](https://github.com/eiserv/frameshift/commit/f25fbc3157f2b8fe2cce81c55719a36d355dd45c))
* **syntax:** refine Sass numeric parsing and default export recovery ([#41](https://github.com/eiserv/frameshift/issues/41)) ([49b08a2](https://github.com/eiserv/frameshift/commit/49b08a26b2627467c270a01c794e397fc512223b))
* **syntax:** structure Sass extension targets and at-root queries ([#36](https://github.com/eiserv/frameshift/issues/36)) ([1f7fade](https://github.com/eiserv/frameshift/commit/1f7fadeb66a6b6a45f66aff6d5e60821bd2d92d9))
* **syntax:** structure Sass imports and validate their URLs ([#34](https://github.com/eiserv/frameshift/issues/34)) ([a6c7f1b](https://github.com/eiserv/frameshift/commit/a6c7f1ba4b8e39967631b9f5bb3c8f0892c745a6))
* **syntax:** structure Sass media queries and supports conditions ([#35](https://github.com/eiserv/frameshift/issues/35)) ([126480a](https://github.com/eiserv/frameshift/commit/126480a7578ee9afdd04efd5cea53fdff2591209))
* **syntax:** validate Sass module rules and preserve their clauses ([#33](https://github.com/eiserv/frameshift/issues/33)) ([1b2521e](https://github.com/eiserv/frameshift/commit/1b2521ee4e113636b3c38e82bf3e17a380eb3503))


### Bug Fixes

* **features:** recognize static-compatible segment modes ([#62](https://github.com/eiserv/frameshift/issues/62)) ([12a9f6a](https://github.com/eiserv/frameshift/commit/12a9f6a4c6c77aa2a57c396124cc9892784f8d51))
* **inspect:** close M2 with adversarial fixtures and detection fixes ([#26](https://github.com/eiserv/frameshift/issues/26)) ([b8c1db1](https://github.com/eiserv/frameshift/commit/b8c1db12b293c0a43ffd55cee285cf4e7b63ea01))
* **modules:** judge inherited moduleResolution after combining configurations ([#58](https://github.com/eiserv/frameshift/issues/58)) ([9a0078a](https://github.com/eiserv/frameshift/commit/9a0078a26fd2a8ead65f3b814b8c0358f5c82b20))
* **syntax:** distinguish raw Sass URLs from function calls ([#39](https://github.com/eiserv/frameshift/issues/39)) ([865f2b7](https://github.com/eiserv/frameshift/commit/865f2b733b2180fff1e6a9dfe797909de96ddae4))
* **syntax:** preserve Sass operator kinds and source spans ([#50](https://github.com/eiserv/frameshift/issues/50)) ([82e8ef3](https://github.com/eiserv/frameshift/commit/82e8ef3dbdf8dc4aaf0538a576f0fbbfb8c4d169))

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
