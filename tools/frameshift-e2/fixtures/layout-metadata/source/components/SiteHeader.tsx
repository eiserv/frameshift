import Link from 'next/link';

export default function SiteHeader() {
  return (
    <header className="site-header">
      <Link href="/" className="brand">
        Lumen Studio
      </Link>
      <Link href="/about">About</Link>
    </header>
  );
}
