export default function DocsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="docs">
      <nav>Documentation</nav>
      <article>{children}</article>
    </div>
  );
}
