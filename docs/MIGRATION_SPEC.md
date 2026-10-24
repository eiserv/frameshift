# Migration Specification: Next.js to Astro + Preact

**Status: specification.** No migration rule is implemented. The output layer converts the JSX subset of section 9.3 and prints Astro components, which experiment E2 applied to fictional fixtures ([IMPLEMENTATION.md](IMPLEMENTATION.md), sections 10 and 11). This document specifies the first migration path, `nextjs-to-astro-preact` (`D-007`). Framework facts reflect the state of October 2026 and must be re-verified when the corresponding rules are implemented. Items marked as proposals are not yet decided: see [DECISIONS.md](DECISIONS.md).

## 1. Purpose and goals

The target users are developers with websites built on Next.js that do not need Next.js: mostly static sites with no meaningful request-time server logic. Next.js static export and Astro can both be hosted statically, so removing a Node.js server is not the main point. The goals of the migration are:

- **Less client-side JavaScript.** Static components become Astro components that ship no JavaScript. Only genuinely interactive components become Preact islands.
- **Interactivity preserved.** Client components keep working as Preact islands.
- **Simpler architecture.** A maintainable Astro project, not a mechanical translation.
- **Behavior preserved.** Routes, content, appearance, accessibility, and behavior stay the same.
- **Static deployment.** The output is a static Astro build whenever the source's functionality allows it.

Non-goals for this path in V1:

- Migrating request-time server logic (middleware or proxy, server actions, request-dependent route handlers, cookies, headers).
- Producing an Astro project that uses a server adapter (SSR output).
- Upgrading unrelated tooling, such as moving from Tailwind CSS v3 to v4 or converting ESLint configurations. Frameshift migrates frameworks, not unrelated tool versions.
- Restructuring code for style (for example, converting CSS Modules to Astro scoped styles). Such changes are optimizations and are opt-in at best.

## 2. Scope

### 2.1 Source

| Aspect | V1 scope |
| --- | --- |
| Framework | Next.js with the App Router. Pages Router projects are detected and analyzed, but their transformation is deferred to V2 (`DF-007`). |
| Versions | Next.js 14.x to 16.x are the validated range for V1 (`D-023`). Projects outside this range are analyzed with a warning, and version-dependent rules behave conservatively. |
| Language | TypeScript and JavaScript, with JSX or TSX. |
| React | React 18 and 19 source code. React 19-only APIs are classified individually (section 9.6). |
| Structure | Single-package projects. A package inside a monorepo can be selected; sibling workspace packages are treated as external dependencies (`D-022`). |
| Rendering | Statically renderable content. Request-time features are detected and classified `unsupported`. |

At the time of writing, Next.js 16 is the current major version (16.3 was released in August 2026). Relevant facts for this path:

- `params` and `searchParams` page props are Promises since Next.js 15. Synchronous access was removed in Next.js 16. Rules must handle both forms.
- `middleware.ts` is deprecated in favor of `proxy.ts` in Next.js 16. Both are detected.
- `next lint`, AMP, and `serverRuntimeConfig`/`publicRuntimeConfig` were removed in Next.js 16.
- Static export (`output: 'export'`) does not support dynamic routes without `generateStaticParams`, request-dependent route handlers, cookies, rewrites, redirects, headers, proxy, ISR, default-loader image optimization, draft mode, server actions, or intercepting routes. A project that already uses static export therefore already avoids most unsupported features.

### 2.2 Target

| Aspect | V1 scope |
| --- | --- |
| Framework | Astro with the `@astrojs/preact` integration. |
| Output | Static (`output: 'static'`). No server adapter. |
| Islands | Preact components hydrated with Astro client directives. |
| Target profile | `astro7-preact10`. It pins tested versions of Astro 7.x, `@astrojs/preact`, Preact 10.x, and `@preact/compat`. Each release supports one profile for the current Astro major. Preact 11 has been stable since 2026-09-30, but `@astrojs/preact` 6.0.6 still declares `preact` `^10.6.5` as its peer, so Preact 11 is adopted only once it is officially supported and passes every island fixture (`D-028`). E2 used `astro` 7.3.8, `@astrojs/preact` 6.0.6, `preact` 10.29.8, and `@preact/compat` 18.3.2 (`D-048`). |

Astro 7 (2026) uses a Rust-based `.astro` compiler built on oxc and Lightning CSS, runs on Vite 8 with Rolldown, applies JSX whitespace rules in templates, and turns unclosed tags into errors instead of repairing them. Frameshift's printer must therefore emit strictly well-formed templates. The JSX-style whitespace handling is helpful here, because it makes React and Astro template semantics closer. E2 confirmed it with Astro 7.3.8, except that `pre` content keeps its whitespace, and found that character references pass through to HTML, which decodes names that JSX keeps literal, and that Astro renders a child `true`, numeric `style` values, and boolean values of non-boolean attributes differently from React ([IMPLEMENTATION.md](IMPLEMENTATION.md), section 10).

## 3. Classification and strategies

Every feature occurrence receives one assessment status. The status model and metrics are defined in [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md).

| Status | Meaning | Strategy |
| --- | --- | --- |
| `auto` | A rule handles the construct deterministically, and the mapping is behavior-preserving under preconditions the engine has verified statically. | Transform. |
| `review` | A rule transforms deterministically, but correctness depends on facts the engine cannot verify statically, or the mapping changes a behavior that may matter. | Transform, and add a review item with a specific checklist. |
| `unsupported` | The engine recognizes the construct and knows that V1 cannot migrate it. | Preserve the source in `.frameshift/unmigrated/`, emit a finding with explanation and remediation, and add a manual task. |
| `unknown` | The engine cannot determine what the construct is or whether it is safe. | Preserve, emit a finding, and ask for human assessment. Never guess. |

Anything not listed in the compatibility matrix (section 17) is `unknown` by definition.

## 4. Guiding principles for this path

1. **Equivalence first, optimization explicit.** Every default mapping aims to preserve behavior. Mappings that knowingly change observable behavior to improve the result (later hydration, image re-encoding, removing hydration from components that look static) are optimizations. They are opt-in and reported separately (see section 21 of [ARCHITECTURE.md](ARCHITECTURE.md)).
2. **Build-time code stays build-time code.** Server components in a static Next.js build run during `next build`. Astro frontmatter runs during `astro build`. Code moves from one to the other with the same execution timing.
3. **Mirror the source's boundaries.** Next.js server and client boundaries map to Astro's static and island boundaries. Server components render to static HTML, client components hydrate. Constraints that already apply at the Next.js boundary (serializable props, server children passed into client components) map onto equivalent Astro constraints.
4. **Idiomatic structure.** Code moves into the conventional Astro layout under `src/`. Data read at build time stays where its filesystem paths expect it (section 16).
5. **Never execute project code.** Configuration files and data are evaluated statically or moved unchanged into build-time code. They are never run during migration.

## 5. Project analysis

### 5.1 Detection

Implemented (`D-074`, section 31 of [IMPLEMENTATION.md](IMPLEMENTATION.md)). A package is classified as Next.js when `next` is a production, development, or optional dependency and at least one structural signal confirms it: a `next.config.*` file, searched upward from the package directory as Next.js does, or an `app/` or `pages/` directory with route files. Next.js uses `app/` and `pages/` in the package directory and falls back to `src/app/` and `src/pages/` for each name separately; from Next.js 15 it rejects the two in different directories. The router type follows from route files with the default page extensions: `page` and `route` files outside private `_` folders in the App Router directory, and page files other than `_app`, `_document`, and `_error` in the Pages Router directory. The version comes from the root lockfile, or else is the lower bound of the declared range and reported as imprecise. In a hybrid project, the App Router part is migrated in V1 and every Pages Router route is `unsupported` with a pointer to V2.

### 5.2 Next.js configuration

`next.config.js`, `.mjs`, and `.ts` are executable modules. Frameshift never executes them. The static evaluator extracts the exported configuration object where it is a literal or built from constants. Each recognized key has a mapping:

| Key | Mapping | Status |
| --- | --- | --- |
| `output: 'export'` | Astro `output: 'static'` (the default) | `auto` |
| `trailingSlash` | `false`: Astro `trailingSlash: 'never'` with `build.format: 'file'`. `true`: `trailingSlash: 'always'` with `build.format: 'directory'`. This keeps the emitted file layout of the static export. | `auto` |
| `basePath` | Astro `base`. Internal links produced from `next/link` receive the base prefix. | `auto` for static hrefs, `review` for computed hrefs |
| `images.unoptimized` | Selects the preserving image strategy (section 12.3) | `auto` |
| `images.remotePatterns`, `images.domains` | Astro `image.remotePatterns`, `image.domains` | `review` |
| `images.loader`, `images.loaderFile` | Custom loader | `unsupported` in V1 |
| `env` | Vite `define` entries in the Astro configuration | `review` |
| `redirects()` returning a literal list of simple redirects | Astro `redirects`. Static Astro output implements these as HTML pages with meta refresh, which differs from HTTP redirects. | `review` |
| `rewrites()`, `headers()` | Host-level configuration | `unsupported`, manual task |
| `reactStrictMode`, `poweredByHeader`, `compress`, `typescript`, `eslint` | No equivalent needed. Dropped with an informational finding. | `auto` |
| `transpilePackages` | Vite `ssr.noExternal` | `review` |
| `webpack` function, `turbopack` rules | Bundler customization, no deterministic mapping to Vite | `unsupported` |
| `pageExtensions` including `mdx` | MDX pages | `unsupported` in V1 |
| `i18n` | Pages Router internationalized routing | `unsupported` |
| `experimental.*` and any unrecognized key | | `unknown` |
| Expressions that cannot be evaluated statically | | `unknown` |

### 5.3 TypeScript and JavaScript configuration

`tsconfig.json` or `jsconfig.json` (parsed as JSONC) is mapped as follows:

- `extends` becomes `astro/tsconfigs/strict` when the source has `strict: true`, otherwise `astro/tsconfigs/base`.
- `jsx` becomes `react-jsx` with `jsxImportSource: "preact"`, as the Preact integration requires.
- Options that affect type checking or resolution and are not covered by the base are kept: `paths`, `baseUrl`, `allowJs`, `noUncheckedIndexedAccess`, and similar.
- Next.js-specific entries are removed: the `next` plugin, `incremental`, `next-env.d.ts`, and `.next/types`.
- `include` becomes `[".astro/types.d.ts", "**/*"]`. `exclude` contains `dist` and `.frameshift`, so preserved unmigrated sources are not type-checked.

Path aliases keep their keys and are retargeted to the moved code (section 16.1). Status: `auto`, with informational findings for dropped options.

### 5.4 Package manager, dependencies, and lockfiles

- **Detection.** As specified in section 7 of [ARCHITECTURE.md](ARCHITECTURE.md). Supported lockfiles: npm (`package-lock.json`), pnpm (`pnpm-lock.yaml`), Yarn classic and Berry (`yarn.lock`), and Bun's text format (`bun.lock`). Bun's binary `bun.lockb` is detected but not parsed (finding `FS1010`). Resolved versions then fall back to declared ranges. The detection rules, the lockfile format versions that are read (npm 2 and 3, Bun text 1 and 2, pnpm 9.0), and their findings are decided in `D-069`; the npm, Bun, and pnpm 9.0 models exist (`D-071`), and so do the Yarn classic and Berry models, which read classic version 1 and Berry versions 4, 5, 6, 8, 9, and 10 (`D-072`).
- **Version facts.** The resolved versions of `next`, `react`, and libraries select version-dependent rules and knowledge base entries.
- **Peer metadata.** Lockfiles that record peer dependencies (npm v3, pnpm v9) supply hints such as "this package has a peer dependency on `next`". Hints never upgrade a package to `compatible`. They only make an `unknown` finding more informative.
- **Output manifest.** The output `package.json` keeps all unrelated fields and the declared ranges of retained dependencies. It removes `next`, `react-dom` (replaced by an alias), `@types/react`, `@types/react-dom`, and `eslint-config-next`. It adds `astro`, `@astrojs/preact`, and `preact`, plus `react` and `react-dom` aliased to `@preact/compat`, at target profile versions. It does not add `"type": "module"`, because that would change how existing `.js` configuration files are interpreted.
- **Lockfile (`P-010`).** Frameshift never generates a lockfile, because that requires resolving packages against a registry. The source lockfile is copied unchanged, so the package manager can keep existing resolutions for retained packages on the next install. The report states that the lockfile is stale, and a manual task asks the user to run an install.
- **Scripts.** `next dev` becomes `astro dev`, `next build` becomes `astro build`, and `next start` becomes `astro preview` with a `review` finding: preview is a local server, not a production server. Scripts that invoke `next` in other ways are `review`. All other scripts are kept unchanged.

### 5.5 Environment variables

Environment files are never read and never copied (see [SECURITY.md](SECURITY.md)). Analysis uses only references in code. The implemented environment classification (`D-096`, section 46 of [IMPLEMENTATION.md](IMPLEMENTATION.md)) records these references, including aliases and destructuring of `process.env`, and whether each module runs during the build, in the browser, or both; the mappings below remain planned.

| Source reference | Mapping | Status |
| --- | --- | --- |
| `process.env.X` in build-time code | `import.meta.env.X` | `auto` |
| `process.env.NEXT_PUBLIC_X` in client code | Depends on the naming strategy below | `review` |
| `process.env.NODE_ENV` | Unchanged; behavior confirmed at build-level verification | `review` |
| Computed access, such as `process.env[name]` | | `unknown` |

Naming of public variables (`D-021`): by default, `process.env.NEXT_PUBLIC_X` becomes `import.meta.env.PUBLIC_X`, and the report lists every variable that must be renamed in `.env` files and hosting settings. With `--env-names keep` (or `envNames: "keep"` in `frameshift.jsonc`), the names stay unchanged (`import.meta.env.NEXT_PUBLIC_X`) and `NEXT_PUBLIC_` is added to Vite's `envPrefix`, which avoids deployment changes. That option is validated against each target profile in M8, and rejected with a clear error if a profile cannot support it.

## 6. Routing

### 6.1 Route mapping

| Source (App Router) | Output | Status |
| --- | --- | --- |
| `app/page.tsx` | `src/pages/index.astro` | `auto` |
| `app/about/page.tsx` | `src/pages/about.astro` | `auto` |
| Nested segments | Nested directories under `src/pages/` | `auto` |
| Route groups, such as `app/(marketing)/pricing/page.tsx` | Group segment removed: `src/pages/pricing.astro`. The group's layouts remain in the layout chain. | `auto` |
| Private folders (`_components/`) | Not routes. Contents are relocated as modules (section 16). | `auto` |
| `src/app/` variant | Same rules | `auto` |

### 6.2 Dynamic routes

- **`[param]` with `generateStaticParams`.** Becomes `[param].astro` with `getStaticPaths()`. Astro executes `getStaticPaths` in an isolated scope, so it may reference only imports. The rule therefore places the original `generateStaticParams` body inside `getStaticPaths` and wraps each returned params object as `{ params }`. Precondition: the function references only imports, globals, and its own locals. Otherwise the referenced declarations would have to move into a generated module, which is `review`. Single-level: `auto`.
- **Parent-dependent parameters.** For a child segment's `generateStaticParams({ params })` that depends on a parent's parameters, the rule generates a composition loop over the parent and child functions. Status `review`.
- **Page parameters.** `const { slug } = await params` (Next.js 15 and later) or `params.slug` (Next.js 14) becomes `Astro.params`. `auto` for these patterns, `review` for other uses of `params`.
- **Catch-all `[...slug]`.** Becomes `[...slug].astro`. Next.js provides `string[]`, while Astro provides a `/`-joined string. The rule joins arrays in `getStaticPaths` and restores the array with `split('/')` where the page reads the parameter. `auto` when the parameter is destructured in a recognized pattern, otherwise `review`.
- **Optional catch-all `[[...slug]]`.** Becomes `[...slug].astro`, where an `undefined` parameter produces the index path. Status `review`.
- **`dynamicParams`.** Irrelevant for static output. Dropped with an informational finding.
- **Dynamic segment without `generateStaticParams`.** The parameter values cannot be enumerated at build time. `unsupported` (`FS3110`).

### 6.3 Special files and unsupported routing features

| Source | Mapping | Status |
| --- | --- | --- |
| Root `not-found.tsx` | `src/pages/404.astro` | `auto` |
| Nested `not-found.tsx` | Static hosts usually serve one 404 page. The nested page is converted to a component and reported. | `review` |
| `loading.tsx` | Dropped. A static build has no streaming, and Astro's default multi-page navigation shows no route loading state. | `review` |
| `error.tsx`, `global-error.tsx` | Runtime error boundaries have no direct equivalent. Dropped with a checklist item. | `review` |
| `template.tsx` | Treated as a layout. Every multi-page navigation remounts, which matches template semantics. | `review` |
| Parallel routes (`@slot`) and `default.tsx` | | `unsupported` |
| Intercepting routes (`(.)`, `(..)`, `(...)`) | | `unsupported` |
| `route.ts` exporting only `GET`, not reading the request, static (`dynamic = 'force-static'` or statically exportable) | Astro static endpoint, such as `src/pages/feed.xml.ts` exporting `GET`. `NextResponse.json` becomes `Response.json`. | `review` |
| `route.ts` with other methods or request-dependent logic | | `unsupported` |
| `proxy.ts`, `middleware.ts` | Every route matched by its `matcher` (all routes if the matcher cannot be evaluated) receives finding `FS3130`. | `unsupported` |

### 6.4 Navigation model

Next.js performs client-side navigation between routes and preserves client component state in shared layouts. Astro's default is multi-page navigation, with a full page load per route. Every project with client components gets the project-level finding `FS3210` (`review`): state in layout-level islands, such as an open menu or a media player, resets on navigation. Astro's client router (view transitions with persisted islands) can approximate the source behavior. Support for it is deferred (`DF-010`).

## 7. Layouts

- Each layout becomes an Astro layout in `src/layouts/`, named from its route position: `RootLayout.astro`, `BlogLayout.astro`, and so on. Name collisions get a deterministic suffix.
- `{children}` becomes `<slot />`.
- A nested layout wraps its content in its parent layout. Each page renders only its nearest layout, which mirrors the directory nesting of the App Router.
- Multiple root layouts in route groups map to multiple Astro root layouts.
- The root layout owns `<html>`, `<head>`, and `<body>`. Next.js adds `<meta charset="utf-8">` and a default viewport meta tag automatically. The generated root layout emits them explicitly so the output matches.
- Metadata flows from page to layout as props (section 13).
- A root layout must be convertible to an Astro component. A root layout that is not convertible is `unsupported` in V1. A nested layout that is not convertible can stay a Preact component rendered without hydration, and receives the page content as static `children`.

## 8. Rendering, data, and server features

| Construct | Mapping | Status |
| --- | --- | --- |
| Synchronous server component | Astro component or static Preact component (section 9) | `auto` |
| Async server component with build-time data fetching | Astro component with top-level `await` in the frontmatter. Async function components are a React Server Components feature with no Preact equivalent, so an async component that cannot become an Astro component is `unsupported`. | `auto` |
| `fetch` without Next.js options | Unchanged. Next.js deduplicates identical requests during a render pass, and Astro does not, which is reported as information. | `auto` |
| `fetch` with `next: { revalidate }` or `cache: 'no-store'` | The Next.js option is removed. The data becomes a build-time snapshot. | `review` |
| Segment `revalidate` (ISR) | Content is fixed at build time and needs rebuilds to refresh | `review` |
| `dynamic = 'force-dynamic'` | | `unsupported` |
| `cookies()`, `headers()`, `draftMode()` | Request-time APIs (`FS3101`, `FS3102`) | `unsupported` |
| `searchParams` in a server page | Not available at build time | `unsupported` |
| Server Actions (`'use server'`) | No server in static output (`FS3120`) | `unsupported` |
| `import 'server-only'` | Removed. Astro frontmatter is server-only by construction. | `auto` |
| `import 'client-only'` | Removed. The importing module becomes part of an island. | `auto` |
| React `cache()`, `unstable_cache` | | `review` |
| `<Suspense>` in server-rendered trees | Unwrapped. A static build resolves all content before writing HTML, as Next.js static export does. | `auto` |
| `export const runtime` | Irrelevant for static output. Dropped. | `auto` |
| Node.js APIs in build-time code (`fs`, `path`) | Unchanged. Content directories keep their relative paths, so paths relative to `process.cwd()` stay valid. | `auto` |

## 9. Components

### 9.1 Conversion ladder

Each component receives exactly one target strategy (`P-007`). The ladder is tried top to bottom:

1. **Astro component** (`.astro`): zero client JavaScript, idiomatic output.
2. **Static Preact component:** the TSX or JSX file is kept, with span edits applied, and rendered by Astro at build time without a client directive. This also produces zero client JavaScript. It is used for server-context components that do not meet the Astro preconditions.
3. **Preact island:** hydrated with a client directive. Used for client-context components at a client boundary.
4. **Preserve:** the component is `unsupported` or `unknown`. Its file goes to `.frameshift/unmigrated/` with findings.

Steps 1 and 2 are both behavior-preserving for static rendering. Step 2 exists so that V1 can stay simple without breaking behavior: anything outside the narrow Astro conversion subset still works.

### 9.2 Render contexts

Analysis computes each component's render contexts (section 9 of [ARCHITECTURE.md](ARCHITECTURE.md)). Module-level contexts are implemented (`D-092`, section 40 of [IMPLEMENTATION.md](IMPLEMENTATION.md)), and the component graph that relates render sites to definitions exists (`D-096`, section 45); per-component contexts follow in M5 from both:

- **Server only:** a candidate for step 1.
- **Client** (inside a `'use client'` module, or reachable from one through imports): step 3 if the component is a topmost client boundary rendered from server context, otherwise part of an island's internal tree.
- **Both:** step 2 for server usages and part of islands for client usages. It stays a single Preact component.

Astro components cannot be rendered by Preact components. Conversion to Astro is therefore computed as a fixed point. Start with every server-only candidate that meets the preconditions. Demote any candidate that is rendered by a component that is not an Astro component. Repeat until nothing changes. The result is deterministic.

### 9.3 Preconditions for Astro conversion (V1)

All of the following must hold:

1. A function component (declaration or arrow function), not a class.
2. Components are decided individually, even when several share a file (`D-025`). Each converted component becomes its own `.astro` file next to the original module, named after the component. Remaining components and non-component exports stay in the original module, because Astro components cannot export values. Module-local helpers used by converted components move into a sibling helper module that both sides import. All consumers' imports are rewritten.
3. Props are a destructuring pattern or a single props identifier. Rest elements are allowed.
4. The body consists of statements followed by a single final `return` of JSX. No early returns.
5. The body does not use React element APIs (`Children`, `cloneElement`, `isValidElement`, `createElement`).
6. The JSX uses only supported constructs: intrinsic elements, fragments, expression containers, `&&` and ternary conditionals, `.map()` callbacks returning JSX, spread attributes, `className` and `htmlFor` (converted to `class` and `for`), `style` objects, `dangerouslySetInnerHTML` (converted to `set:html`), and `key` (removed). `children` may be used only as a direct JSX child expression and becomes `<slot />`. The implemented conversion ([IMPLEMENTATION.md](IMPLEMENTATION.md), section 10) narrows this list where React and Astro render differently: `style` objects need string values, attribute strings must not contain line breaks, comments inside tags are not converted, and value-dependent differences are returned as assumptions that rules classify as `review` unless they can prove them.
7. Every JSX element type is an intrinsic element or a project component whose own strategy is known. Third-party components keep the containing component on step 2.
8. Async functions are allowed.

### 9.4 Islands and hydration

- **Island selection.** Each topmost client component rendered from a server context becomes one island. Client components rendered by other client components are part of that island's tree. This mirrors the Next.js client boundaries.
- **Default directive.** `client:load`, which hydrates immediately on page load and is closest to Next.js hydration timing (`P-009`).
- **`next/dynamic` with `ssr: false`.** Becomes `client:only="preact"`. The `loading` option becomes the island's fallback slot content.
- **Optimizations (not in V1).** `client:idle`, `client:visible`, and `client:media` change hydration timing. They are optimizations: they may later be offered as opt-in suggestions, and reports will list candidates.
- **Unnecessary `'use client'`.** A client component with no state, effects, event handlers, refs, browser APIs, or context consumption stays an island by default. Removing its hydration is an optimization.
- **`'use client'` page.** The page becomes a thin Astro page that renders the page component as one island. This is behavior-preserving but reduces no JavaScript. Status `review`, with a suggestion.

### 9.5 Props across the static-to-island boundary

Astro serializes island props. It supports plain objects, arrays, strings, numbers, `Date`, `Map`, `Set`, `RegExp`, `BigInt`, `URL`, typed arrays, and `Infinity`. Next.js already restricts props from server to client components to serializable values, so most cases map directly.

| Prop passed from server to client | Mapping | Status |
| --- | --- | --- |
| Literals, plain data, supported built-in types | Unchanged | `auto` |
| `children` | Default slot. Astro passes it as static HTML, matching React Server Components semantics. | `auto` |
| JSX element in another prop, such as `icon={<Star />}` | Named slot (`<Star slot="icon" />`). Astro passes named slots to framework components as props. | `review` |
| Promise | Not serializable by Astro | `unsupported` |
| Server Action | | `unsupported` |
| Values whose type cannot be determined statically | | `review` |

### 9.6 Hooks and React APIs

Islands and static Preact components use the `preact/compat` strategy by default (`P-008`). Source imports from `react` stay unchanged, and `react` and `react-dom` resolve to `@preact/compat` through package aliases and the integration's `compat: true` option. This minimizes code changes. It also keeps React semantics where native Preact differs, for example `onChange` on text inputs, event naming, and SVG attribute names. Rewriting to native Preact imports (`preact/hooks`) is a later optimization. It will be available only when every used API, attribute, and event has a verified mapping in the knowledge base.

| Construct | Status |
| --- | --- |
| `useState`, `useEffect`, `useLayoutEffect`, `useRef`, `useMemo`, `useCallback`, `useReducer`, `useContext`, `createContext`, `useImperativeHandle` | `auto` |
| `memo`, `forwardRef`, `lazy`, client-side `Suspense`, `createPortal`, class components, error boundaries | `auto` (via compat) |
| `useTransition`, `useDeferredValue`, `useSyncExternalStore`, `useInsertionEffect` | `auto` if the compat coverage table for the pinned Preact version confirms support, otherwise `review` |
| `useId` | `review`: ID generation is per island root, so uniqueness across islands must be checked |
| `use`, `useActionState`, `useOptimistic`, `useFormStatus`, `useEffectEvent` (React 19 and later) | `unsupported` unless the pinned Preact version is verified to provide them. Available sources indicate that `preact/compat` does not support `use()` at the time of writing. |
| `React.StrictMode` | Removed with an informational finding (`auto`) |
| `<title>`, `<meta>`, `<link>` rendered inside components (React 19 hoisting) | `review`: Preact does not hoist these into `<head>` |
| Custom hooks | Analyzed recursively. A custom hook inherits the most severe status of what it uses. |

### 9.7 Context providers and shared state

Astro islands are independent render roots. React context does not cross islands, and static content between islands is not inside any provider.

| Situation | Mapping | Status |
| --- | --- | --- |
| Provider and all consumers inside one island's tree | Unchanged | `auto` |
| Provider in a layout (for example, `app/providers.tsx`) with no consumers in client code | Provider removed | `review` |
| Provider whose consumers span several islands | Requires a shared store, for example module-level state shared between islands. Planned for V2. | `unsupported` in V1 (`FS3201`) |
| State that must survive navigation | Resets on every page load in multi-page navigation (`FS3210`) | `review` |

## 10. Next.js APIs

| API | Mapping | Status |
| --- | --- | --- |
| `next/link` with string or literal object `href` | `<a href>`. Object hrefs are serialized deterministically. The `basePath` prefix is applied. | `auto` |
| `next/link` with computed `href` | `<a href={...}>`, with a base prefix expression if `basePath` is set | `review` |
| `next/link` props `prefetch`, `replace`, `scroll` | Dropped. Astro prefetching is an optional optimization. | `review` |
| `next/image` | See section 12.3 | `auto` or `review` |
| `next/font/google`, `next/font/local` | See section 12.2 | `review` |
| `next/script` | `strategy="afterInteractive"` (the default) becomes an inline external script with `async`. `beforeInteractive` moves into `<head>`. `lazyOnload` and `onLoad` handlers need hand-written loaders. `worker` (Partytown) is `unsupported`. | `review` |
| `next/dynamic` | `ssr: false` becomes `client:only="preact"`. Other uses become regular imports, because Astro splits islands into separate bundles anyway. | `review` |
| `next/navigation`: `useRouter`, `usePathname`, `useSearchParams`, `useParams` | A generated helper in `src/frameshift-compat/` implements the subset needed for static multi-page sites. `push` and `replace` map to `location` methods. `usePathname` and `useParams` use values passed from the page during server rendering, and `useSearchParams` reads `location.search` on the client. | `review` |
| `redirect()`, `permanentRedirect()` in server code | Static output has no request-time redirects | `unsupported` |
| `notFound()` in server code | Dropped where it guards parameters that `getStaticPaths` already restricts. Otherwise flagged. | `review` |
| `next/headers`, `next/cache`, `next/og`, `next/server` (outside static route handlers), `next/form` | | `unsupported` |
| `next/head`, `next/router`, `next/document`, `next/app` | Pages Router (`DF-007`) | `unsupported` |
| `next/legacy/image` | Treated like `next/image` with legacy layout props | `review` |
| `next/config` | Removed in Next.js 16 | `unsupported` |
| `@next/third-parties` components | Knowledge base mappings to inline scripts | `review` |

## 11. Styling

| Construct | Mapping | Status |
| --- | --- | --- |
| Global CSS imported in a layout | Imported in the corresponding Astro layout frontmatter, in the same order | `auto` |
| Global CSS imported elsewhere | The import moves with its module | `auto` |
| CSS Modules (`*.module.css`) | Unchanged. Vite supports CSS Modules in Astro and Preact files, including `composes` and `:global`. Generated class name formats differ. | `auto` |
| Sass and SCSS (global and modules) | Unchanged. The `sass` dependency is kept. | `auto` |
| Tailwind CSS v4 through PostCSS (`@tailwindcss/postcss`, `@import "tailwindcss"`) | The PostCSS setup is kept, which preserves the source tooling. Automatic source detection includes `.astro` files. Moving to `@tailwindcss/vite` is optional and not part of the default. | `auto` |
| Tailwind CSS v3 (`tailwind.config.*`, `@tailwind` directives) | PostCSS setup kept. `content` globs are extended to include `.astro` files and relocated paths when the configuration evaluates statically, otherwise `review`. | `auto` or `review` |
| PostCSS configuration | Kept. The object form that Next.js uses is also supported by Vite's PostCSS loading. | `auto` |
| `styled-jsx` (`<style jsx>`) | Candidate for conversion to Astro scoped styles in V2 | `unsupported` |
| Runtime CSS-in-JS (styled-components, Emotion) | Server-side style registries are Next.js-specific | `unsupported` |
| Cascade order across the whole page | Next.js and Vite may order stylesheets differently. Static output comparison cannot detect cascade differences (section 16 of [ARCHITECTURE.md](ARCHITECTURE.md)). | `review` (project-level) |

## 12. Assets, fonts, and images

### 12.1 Assets

- `public/` is copied verbatim and keeps the same URL semantics. Files are counted as verbatim inventory items (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)).
- Static imports of images and other assets keep working: Vite returns an object with `src`, `width`, and `height` for images, which matches the fields code commonly reads from Next.js `StaticImageData`. Code that reads `blurDataURL` is `review`.
- Static metadata files in `app/` (`favicon.ico`, `icon.*`, `apple-icon.*`, `opengraph-image.*`, `twitter-image.*`, `robots.txt`, `sitemap.xml`, `manifest.json` or `.webmanifest`) are copied to `public/`. The corresponding `<link>` and `<meta>` tags are added to the root layout. Status `auto`.
- Generated metadata files (`robots.ts`, `sitemap.ts`, `manifest.ts`) become Astro static endpoints (`review`). Generated images (`opengraph-image.tsx` using `next/og`) are `unsupported`.
- SVG files imported as React components through a custom webpack loader depend on the `webpack` configuration and are `unsupported`.

### 12.2 Fonts

`next/font` downloads Google fonts at build time, self-hosts them, and generates fallback font metrics. The rule maps calls with literal options (family, weights, subsets, `variable`, `display`) to Astro's built-in Fonts API if the pinned target version provides it in stable form, and otherwise to self-hosted `@font-face` CSS. Usages of `className` and `variable` are rewritten to generated classes and CSS variables. Fallback metric adjustment may differ, so status is `review`. Both frameworks need network access at build time to fetch Google fonts. `next/font/local` maps to local font files with generated `@font-face` rules (`review`).

### 12.3 Images

| Situation | Mapping | Status |
| --- | --- | --- |
| Source does not optimize (`images.unoptimized: true`) | `<img>` with the same `src`, dimensions, `alt`, `loading`, `decoding`, `sizes`, and `srcSet`. This preserves the source output. Using `astro:assets` here would be an optimization. | `auto` |
| Source optimizes with the default loader (server deployment), local static import, in an Astro component | `<Image>` from `astro:assets`, because image optimization was part of the original behavior | `auto` |
| Remote `src` | `<Image>` with remote patterns, or `<img>` | `review` |
| `priority` | Eager loading with high fetch priority | `auto` |
| `fill` | Equivalent inline positioning styles | `review` |
| `placeholder="blur"` | No equivalent. Dropped. | `review` |
| Inside an island or static Preact component | `<img>`, because `astro:assets` components cannot render inside Preact. Optimization is lost if the source optimized. | `review` |

## 13. Metadata and SEO

- **Static `metadata` objects** with literal values are resolved at planning time into head tags. Frameshift implements Next.js's documented merge semantics for the supported fields. Metadata objects are merged shallowly from the root layout to the page. A nested object such as `openGraph`, when defined in a child segment, replaces the parent's object. A `title.template` applies to titles in child segments, `title.absolute` ignores templates, and `metadataBase` resolves relative URLs. Status `auto`.
- **Supported fields in V1:** `title` (string, `default`, `template`, `absolute`), `description`, `keywords`, `authors`, `creator`, `publisher`, `robots` (basic directives), `alternates.canonical`, `openGraph` (title, description, url, siteName, images, locale, type), `twitter` (card, title, description, images, creator, site), `icons`, `metadataBase`, and the `viewport` export (width, initial scale, theme color).
- **Fields outside this subset** are not emitted. Each one produces a finding, and the unit is marked `unsupported` for that field. It is never silently dropped.
- **Non-literal `metadata`** (computed values or imports) and **`generateMetadata`** stay in the page frontmatter and are rendered through a generated build-time helper in `src/frameshift-compat/`, which implements the same merge semantics. Status `review`.
- `metadataBase` also sets Astro's `site` option, when it is a literal URL.

## 14. Third-party React libraries

The knowledge base classifies packages for specific version ranges:

| Class | Meaning | Effect on code that uses the package |
| --- | --- | --- |
| `framework` | Packages of the source framework itself (`next`, `react`, `react-dom`, and their type packages) | Replaced or removed as specified in section 5.4 |
| `compatible` | Framework-agnostic, or works natively with Preact | `auto` |
| `compat` | Verified to work through `preact/compat` in the target profile | `auto`, with the compat strategy |
| `compat-unverified` | Expected to work through compat but not verified by fixtures | `review` |
| `incompatible` | Depends on React internals, React Server Components, or Next.js | `unsupported`, with alternatives where known |
| `unknown` | No knowledge base entry | `unknown` |

Rules for this class model:

- **Usage, not declaration.** A package that is declared but never imported affects nothing except the dependency inventory.
- **Server-only usage.** A React library used only in server-context components can still work in static rendering. The containing component stays on step 2 of the ladder (static Preact), so the library renders through compat at build time.
- **Package compatibility claims require evidence.** Each knowledge base entry references fixtures that exercise the package in the target profile. V1 covers a curated set of widely used libraries. The selection is made during milestone M5 (see [ROADMAP.md](ROADMAP.md)).

## 15. Unsupported server-side features

V1 detects these features and explains them, but does not migrate them:

| Feature | Why unsupported in V1 | What Frameshift produces |
| --- | --- | --- |
| `proxy.ts` and `middleware.ts` | Runs on every matched request. Static output has no request handling. | `FS3130` on every matched route. Source preserved. Suggestions such as host redirect rules or edge functions, without generating them. |
| Request-dependent route handlers | Handle requests at runtime | `unsupported` endpoint unit. Source preserved. Manual task. |
| Server Actions | Execute on the server per request | `FS3120` on the action and on every form or call site that uses it. Suggestions: an external form service or a serverless function. A later stage may map them to Astro Actions with a server adapter. |
| `cookies()`, `headers()`, `draftMode()`, `searchParams` in server code, `force-dynamic` | Request-time rendering | `unsupported` route with exact locations |
| ISR (`revalidate`) | Needs a server or a platform cache | `review`: content becomes a build-time snapshot |

A project whose routes mostly depend on these features receives a project-level verdict that explains why it is a poor candidate for a static migration (see [MIGRATION_REPORTING.md](MIGRATION_REPORTING.md)).

## 16. Output project structure

### 16.1 Output layout (`D-024`)

The output follows the conventional Astro project structure, so it reads like a hand-written Astro project.

| Source | Output |
| --- | --- |
| Route files (`app/**/page.*`) | `src/pages/**` per section 6 |
| Layouts (`app/**/layout.*`) | `src/layouts/<Name>.astro` |
| Route handlers | `src/pages/**` endpoints |
| Components colocated in `app/` (including private folders) | `src/components/<route directory>/`, for example `app/blog/_components/PostCard.tsx` becomes `src/components/blog/PostCard.astro` or `.tsx` |
| Non-component modules colocated in `app/` | `src/lib/<route directory>/` |
| Stylesheets colocated in `app/` | `src/styles/<route directory>/` |
| Assets imported by code and colocated in `app/` | `src/assets/<route directory>/` |
| Root-level code directories (`components/`, `lib/`, `hooks/`, `utils/`, `styles/`, and others containing code) | `src/<same name>/` |
| An existing `src/` directory (besides `src/app/`) | Unchanged |
| Non-code data directories read at build time (for example `content/`, `posts/`, `data/`) | Same relative path at the root, so paths relative to `process.cwd()` stay valid. Static imports of such files are rewritten. |
| `public/` and configuration files | Root |

Rules:

- Every import that refers to a moved file is rewritten deterministically. A component converted to an Astro component changes its extension (for example `Button.tsx` to `Button.astro`), and imports of it include the `.astro` extension, which Astro requires.
- Path aliases keep their keys. If all targets of an alias move together, the alias is retargeted (for example `@/*` from `./*` to `./src/*`). Otherwise, affected imports become relative imports.
- Collisions with existing files, including collisions that differ only in case, are detected at planning time (`FS4002`).

### 16.2 Generated and support files

| File | Purpose |
| --- | --- |
| `astro.config.mjs` | Generated from the Next.js configuration mapping (section 5.2) |
| `package.json` | Transformed (section 5.4) |
| `tsconfig.json` | Transformed (section 5.3) |
| `.gitignore` | Source entries kept. `dist/` and `.astro/` added. Next.js entries kept, because they are harmless. |
| `src/frameshift-compat/` | Generated helpers, only when rules need them (navigation helper, metadata helper) |
| `.frameshift/manifest.json` | Provenance (see [ARCHITECTURE.md](ARCHITECTURE.md)) |
| `.frameshift/report.json` | Full machine-readable report |
| `.frameshift/unmigrated/` | Verbatim copies of unmigrated source files under their original relative paths |
| `.frameshift/SUMMARY.md` | Human-readable summary of the migration and remaining manual tasks. The CLI points to it after every migration. |

Removed relative to the source: `next.config.*`, `next-env.d.ts`, and the `app/` directory, whose contents now live under `src/`. Generated source files carry no header comments, so the result reads as a normal Astro project. Provenance lives in the manifest.

## 17. Compatibility matrix

Legend: **A** `auto`, **R** `review`, **U** `unsupported` in V1. "Later" names the stage that plans to support an unsupported feature. Any construct not listed is `unknown`.

Feature IDs use at least two dot-separated lowercase name segments (`D-055`). Existing two-segment names remain stable. Entries ending in `.*` describe catalog families, not literal identifiers; each concrete member must pass `FeatureId` validation. The facade regression test validates every concrete ID and an example expansion of each family in this matrix.

### 17.1 Project and configuration

| Feature | Feature ID | V1 | Notes |
| --- | --- | --- | --- |
| App Router project | `nextjs.project.app-router` | A | Section 5.1 |
| Pages Router project | `nextjs.project.pages-router` | U | Analysis only. Later: V2. |
| Static configuration keys | `nextjs.config.*` | A, R, or U | Per key, section 5.2 |
| Unrecognized or experimental configuration | `nextjs.config.unknown` | Unknown | |
| `tsconfig.json`, `jsconfig.json` | `ts.config` | A | Section 5.3 |
| Package manifest and scripts | `npm.manifest` | A or R | Section 5.4 |
| Lockfiles (npm, pnpm, Yarn, Bun text) | `npm.lockfile` | A | Copied. Install is a manual task. |
| Bun binary lockfile | `npm.lockfile.bun-binary` | R | Not parsed. Declared ranges used instead. |
| Monorepo workspace package | `npm.workspace` | R | `D-022` |
| Environment variable references | `env.reference` | A or R | Section 5.5 |
| Deployment configuration (`vercel.json`, `netlify.toml`, CI workflows) | `deploy.config` | U | Manual tasks (build command, output directory `out/` to `dist/`) |
| Test setup (Jest, Vitest, Playwright) | `test.setup` | Out of scope | Reported separately, not migrated in V1 |

### 17.2 Routing

| Feature | Feature ID | V1 | Notes |
| --- | --- | --- | --- |
| Static page | `nextjs.route.page` | A | |
| Nested segments, route groups, private folders | `nextjs.route.segment`, `nextjs.route.group`, `nextjs.route.private` | A | |
| Dynamic segment with `generateStaticParams` | `nextjs.route.dynamic` | A or R | R for parent-dependent parameters |
| Dynamic segment without `generateStaticParams` | `nextjs.route.dynamic-unenumerable` | U | `FS3110` |
| Catch-all | `nextjs.route.catch-all` | A or R | Parameter shape adaptation |
| Optional catch-all | `nextjs.route.optional-catch-all` | R | |
| Parallel routes | `nextjs.route.parallel` | U | |
| Intercepting routes | `nextjs.route.intercepting` | U | |
| Layouts | `nextjs.file.layout` | A | Non-convertible root layout: U |
| `template` | `nextjs.file.template` | R | |
| `loading` | `nextjs.file.loading` | R | Dropped |
| `error`, `global-error` | `nextjs.file.error` | R | Dropped |
| Root `not-found` | `nextjs.file.not-found` | A | Nested: R |
| Static GET route handler | `nextjs.file.route-handler-static` | R | |
| Other route handlers | `nextjs.file.route-handler` | U | Later: V3 with server adapters |
| `proxy.ts`, `middleware.ts` | `nextjs.file.proxy` | U | `FS3130`. Later: V3. |
| Client-side navigation semantics | `nextjs.navigation.client` | R | `FS3210` |

### 17.3 Rendering and data

| Feature | Feature ID | V1 | Notes |
| --- | --- | --- | --- |
| Server component, synchronous or async | `react.server-component` | A | Async requires Astro conversion |
| Build-time `fetch` | `nextjs.fetch` | A | R with Next.js cache options |
| ISR `revalidate` | `nextjs.segment.revalidate` | R | |
| `force-dynamic` | `nextjs.segment.dynamic` | U | |
| `cookies`, `headers`, `draftMode` | `nextjs.api.request` | U | `FS3101`, `FS3102` |
| Server `searchParams` | `nextjs.api.search-params-server` | U | |
| Server Actions | `nextjs.api.server-action` | U | `FS3120`. Later: V3. |
| `server-only`, `client-only` | `nextjs.guard` | A | Removed |
| React `cache`, `unstable_cache` | `react.cache` | R | |
| Server `Suspense` | `react.suspense-server` | A | Unwrapped |

### 17.4 Components and React

| Feature | Feature ID | V1 | Notes |
| --- | --- | --- | --- |
| Server-only component meeting Astro preconditions | `component.astro-convertible` | A | Ladder step 1 |
| Other server-context component | `component.preact-static` | A | Ladder step 2 |
| Client component at a boundary | `component.island` | A | `client:load` |
| `'use client'` page | `component.client-page` | R | |
| `next/dynamic` with `ssr: false` | `nextjs.api.dynamic-no-ssr` | A | `client:only` |
| Serializable props across the boundary | `boundary.props` | A | |
| JSX props across the boundary | `boundary.jsx-props` | R | Named slots |
| Promise props across the boundary | `boundary.promise-props` | U | |
| Compat-covered hooks and APIs | `react.hook.*`, `react.api.*` | A | Section 9.6 |
| React 19-only hooks | `react.hook.react19` | U | Unless verified |
| `useId` | `react.hook.use-id` | R | |
| Head elements rendered in components | `react.head-hoisting` | R | |
| Context within one island | `react.context.local` | A | |
| Context across islands | `react.context.cross-island` | U | `FS3201`. Later: V2. |

### 17.5 Next.js APIs, styling, assets, metadata

| Feature | Feature ID | V1 | Notes |
| --- | --- | --- | --- |
| `next/link` | `nextjs.api.link` | A or R | |
| `next/image` | `nextjs.api.image` | A or R | Section 12.3 |
| `next/font` | `nextjs.api.font` | R | |
| `next/script` | `nextjs.api.script` | R | `worker` strategy: U |
| `next/navigation` hooks | `nextjs.api.navigation` | R | Generated helper |
| `redirect`, `permanentRedirect` | `nextjs.api.redirect` | U | |
| `notFound` | `nextjs.api.not-found` | R | |
| `next/headers`, `next/cache`, `next/og`, `next/form` | `nextjs.api.server-only` | U | |
| Global CSS, CSS Modules, Sass | `css.global`, `css.module`, `css.sass` | A | |
| Tailwind CSS v3 and v4 | `css.tailwind` | A or R | |
| `styled-jsx` | `css.styled-jsx` | U | Later: V2 |
| Runtime CSS-in-JS | `css.in-js` | U | |
| `public/` assets | `asset.public` | A | Verbatim |
| Static asset imports | `asset.import` | A | |
| Static metadata files | `nextjs.metadata.file` | A | |
| Generated metadata files | `nextjs.metadata.route` | R | `opengraph-image.tsx`: U |
| Static `metadata` (supported fields) | `nextjs.metadata.static` | A | |
| Unsupported metadata fields | `nextjs.metadata.unsupported-field` | U | Per field |
| `generateMetadata`, non-literal metadata | `nextjs.metadata.dynamic` | R | Generated helper |

## 18. Transformation examples

All examples are illustrative. Project names, paths, and content are fictional. Version numbers shown as `<pinned>` are supplied by the target profile.

### 18.1 Root layout and a static page

Source `app/layout.tsx`:

```tsx
import './globals.css';
import type { Metadata } from 'next';
import { SiteHeader } from '@/components/SiteHeader';

export const metadata: Metadata = {
  title: { default: 'Lumen Studio', template: '%s | Lumen Studio' },
  description: 'A small design studio.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-white">
        <SiteHeader />
        <main>{children}</main>
      </body>
    </html>
  );
}
```

Source `app/about/page.tsx`:

```tsx
import Link from 'next/link';

export const metadata = { title: 'About' };

export default function AboutPage() {
  return (
    <section className="prose">
      <h1>About us</h1>
      <p>We build calm websites.</p>
      <Link href="/contact" className="underline">Get in touch</Link>
    </section>
  );
}
```

Output `src/layouts/RootLayout.astro` (`SiteHeader` was converted to an Astro component):

```astro
---
import '../styles/globals.css';
import SiteHeader from '@/components/SiteHeader.astro';

interface Props {
  title: string;
  description: string;
}

const { title, description } = Astro.props;
---
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{title}</title>
    <meta name="description" content={description} />
  </head>
  <body class="min-h-screen bg-white">
    <SiteHeader />
    <main>
      <slot />
    </main>
  </body>
</html>
```

Output `src/pages/about.astro` (metadata resolved statically: the page title is applied to the layout's template, and the description is inherited):

```astro
---
import RootLayout from '../layouts/RootLayout.astro';
---
<RootLayout title="About | Lumen Studio" description="A small design studio.">
  <section class="prose">
    <h1>About us</h1>
    <p>We build calm websites.</p>
    <a href="/contact" class="underline">Get in touch</a>
  </section>
</RootLayout>
```

Rules applied: `layout.chain`, `metadata.static`, `route.static-page`, `link.anchor`, `style.global-import`. All occurrences are `auto`.

### 18.2 Dynamic route with `generateStaticParams`

Source `app/blog/[slug]/page.tsx`:

```tsx
import { getAllPosts, getPost } from '@/lib/posts';

export async function generateStaticParams() {
  const posts = await getAllPosts();
  return posts.map((post) => ({ slug: post.slug }));
}

export default async function PostPage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const post = await getPost(slug);
  return (
    <article>
      <h1>{post.title}</h1>
      <div dangerouslySetInnerHTML={{ __html: post.html }} />
    </article>
  );
}
```

Output `src/pages/blog/[slug].astro`:

```astro
---
import RootLayout from '../../layouts/RootLayout.astro';
import { getAllPosts, getPost } from '@/lib/posts';

export async function getStaticPaths() {
  const generateStaticParams = async () => {
    const posts = await getAllPosts();
    return posts.map((post) => ({ slug: post.slug }));
  };
  return (await generateStaticParams()).map((params) => ({ params }));
}

const { slug } = Astro.params as { slug: string };
const post = await getPost(slug);
---
<RootLayout title="Lumen Studio" description="A small design studio.">
  <article>
    <h1>{post.title}</h1>
    <div set:html={post.html}></div>
  </article>
</RootLayout>
```

The function body is copied as source text, because `getStaticPaths` runs in an isolated scope that can only reference imports. The parameter type comes from the source's declared `params` type. The page defines no metadata, so it inherits the layout's default title, exactly as in Next.js.

### 18.3 Catch-all route parameter adaptation

Source `app/docs/[...slug]/page.tsx`:

```tsx
import { loadDoc } from '@/lib/docs';

export function generateStaticParams() {
  return [{ slug: ['intro'] }, { slug: ['guides', 'setup'] }];
}

export default async function DocPage({ params }: { params: Promise<{ slug: string[] }> }) {
  const { slug } = await params;
  const doc = await loadDoc(slug.join('/'));
  return <article dangerouslySetInnerHTML={{ __html: doc.html }} />;
}
```

Output `src/pages/docs/[...slug].astro` (layout omitted):

```astro
---
import { loadDoc } from '@/lib/docs';

export async function getStaticPaths() {
  const generateStaticParams = () => [{ slug: ['intro'] }, { slug: ['guides', 'setup'] }];
  return generateStaticParams().map((params) => ({
    params: { ...params, slug: params.slug.join('/') },
  }));
}

const slug = (Astro.params as { slug: string }).slug.split('/');
const doc = await loadDoc(slug.join('/'));
---
<article set:html={doc.html}></article>
```

### 18.4 Client component as a Preact island

Source `components/Counter.tsx`:

```tsx
'use client';

import { useState } from 'react';

export default function Counter({ initial = 0 }: { initial?: number }) {
  const [count, setCount] = useState(initial);
  return (
    <button type="button" onClick={() => setCount(count + 1)}>
      Clicked {count} times
    </button>
  );
}
```

Output `src/components/Counter.tsx` (moved into the Astro layout; only the directive is removed):

```tsx
import { useState } from 'react';

export default function Counter({ initial = 0 }: { initial?: number }) {
  const [count, setCount] = useState(initial);
  return (
    <button type="button" onClick={() => setCount(count + 1)}>
      Clicked {count} times
    </button>
  );
}
```

Usage in an Astro page: `<Counter client:load initial={3} />`, imported with `import Counter from '../components/Counter';`.

Configuration effects: `integrations: [preact({ compat: true })]` in `astro.config.mjs`, and `"react": "npm:@preact/compat@<pinned>"` and `"react-dom": "npm:@preact/compat@<pinned>"` in `package.json`. Additional package manager overrides (npm `overrides`, pnpm `pnpm.overrides`, Yarn `resolutions`) make transitive dependencies resolve to compat as well.

### 18.5 JSX passed from a server component to a client component

Source (server page, `Card` is a client component):

```tsx
<Card title="Featured" icon={<StarIcon />}>
  <p>Rendered on the server.</p>
</Card>
```

Output in the Astro page:

```astro
<Card client:load title="Featured">
  <StarIcon slot="icon" />
  <p>Rendered on the server.</p>
</Card>
```

Astro passes the named slot `icon` and the default slot `children` to the Preact component as props containing static HTML. This matches React Server Components semantics, where server-rendered JSX passed to a client component is not interactive. Status `review`: the reviewer confirms that `Card` only renders `icon` and does not inspect or clone it.

### 18.6 Image in a project that does not optimize images

Source (`images: { unoptimized: true }` in `next.config.ts`; the image is colocated in `app/`):

```tsx
import Image from 'next/image';
import hero from './hero.jpg';

<Image src={hero} alt="Studio interior" priority />
```

Output in `src/pages/index.astro`:

```astro
---
import hero from '../assets/hero.jpg';
---
<img src={hero.src} width={hero.width} height={hero.height} alt="Studio interior" loading="eager" fetchpriority="high" decoding="async" />
```

The source served the original file without optimization, so a plain `<img>` preserves behavior. Moving to `astro:assets` would re-encode images and is an optimization.

### 18.7 Configuration

Source `next.config.ts`:

```ts
import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  output: 'export',
  trailingSlash: false,
  images: { unoptimized: true },
  reactStrictMode: true,
};

export default nextConfig;
```

Output `astro.config.mjs`:

```js
import { defineConfig } from 'astro/config';
import preact from '@astrojs/preact';

export default defineConfig({
  output: 'static',
  trailingSlash: 'never',
  build: { format: 'file' },
  integrations: [preact({ compat: true })],
});
```

`reactStrictMode` produces an informational finding. `images.unoptimized` selects the preserving image strategy and needs no Astro configuration.

### 18.8 Unsupported Server Action

Source:

```tsx
// app/contact/page.tsx
import { sendMessage } from './actions';

export default function ContactPage() {
  return (
    <form action={sendMessage}>
      <input name="email" type="email" required />
      <button type="submit">Send</button>
    </form>
  );
}

// app/contact/actions.ts
'use server';

export async function sendMessage(formData: FormData) {
  // ...
}
```

Frameshift output (CLI excerpt, fictional):

```text
blocker  FS3120  Server Action cannot run in static output
         app/contact/page.tsx:5:19  <form action={sendMessage}>
         Server Actions execute on a server for each request. A static Astro site has no server.
         Options: post the form to an external form service or to a serverless function on your host.
         The route /contact was not migrated. Sources preserved in .frameshift/unmigrated/app/contact/.
```

The `/contact` route and `actions.ts` are classified `unsupported`, preserved, and listed as manual tasks in `.frameshift/SUMMARY.md`.

## 19. V1 rule set

The minimum rule set for V1 and its milestone order are defined in [ROADMAP.md](ROADMAP.md). It covers the `A` and `R` entries of section 17, except those marked as later, plus detection and explanation for every `U` entry.

## 20. Known limitations of this path

- Runtime behavior that static output comparison cannot see is not verified in V1: CSS cascade order, hydration timing, focus management, animations.
- The navigation model changes from client-side transitions to full page loads (section 6.4).
- Context shared across islands, Pages Router projects, MDX pages, CSS-in-JS, and all request-time features are unsupported in V1.
- Third-party library compatibility is only as good as knowledge base evidence. Unknown packages remain `unknown`.
- Data fetched at build time from external services is a snapshot, as in a Next.js static export. A server-deployed Next.js application that refreshed data per request or through ISR changes behavior. This is reported, not hidden.
- Frameshift does not run formatters or linters on the output.
- Deployment configuration (hosting settings, CI workflows) is reported but not rewritten.

## 21. Open questions for this path

The questions raised by this path so far are resolved in [DECISIONS.md](DECISIONS.md) (`D-021` to `D-025`, `D-028`). New questions are added there.
