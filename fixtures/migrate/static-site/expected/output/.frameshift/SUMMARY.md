# Frameshift migration summary

Source: Next.js `16.2.1` (App Router). Target: Astro with Preact, profile `astro7-preact10`. Rules: `nextjs-to-astro-preact/migrate-v0`.

Migrate v0 converts only static pages, layouts, and components that the analysis rates `auto` or `review`. Preserved files are copied unchanged below `.frameshift/unmigrated/` and need manual migration. No percentage here claims behavioral equivalence; build and check the result.

## Results

- Converted to Astro components: 8
- Kept as JavaScript or TypeScript with rewritten imports: 2
- Copied unchanged: 3
- Generated project files: 5
- Preserved for manual migration: 2
- Not copied: 3

## Project tasks

- install dependencies with your package manager; lockfiles are not carried over
- 4 Next.js and React dependencies were removed from package.json; check that nothing else needs them
- configuration concern config:source.analysis (review) is not migrated by v0; see `frameshift analyze`

## Preserved files

- `app/blog/[slug]/page.tsx`: preserved for manual migration: the analysis rates this file's units unsupported
- `app/contact/page.tsx`: preserved for manual migration: client components and islands are not converted by migrate v0

## Converted files to review

- `app/page.tsx`: converted; check the result, because the analysis rates it review, and the conversion assumes values that React and Astro render alike (child-not-true)
- `components/SiteHeader.tsx`: converted; check the result, because the analysis rates it review, and the conversion assumes values that React and Astro render alike (child-not-true)

## Not copied

- `next.config.mjs`: replaced by the generated astro.config.mjs
- `package.json`: replaced by the generated package.json
- `tsconfig.json`: replaced by the generated tsconfig.json

Preserved files keep their original package-relative paths below `.frameshift/unmigrated/`.
