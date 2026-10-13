import Link from 'next/link';

export default function HomePage() {
  return (
    <main>
      <h1>Harbor Notes</h1>
      {/* The two sections of this fictional site. */}
      <nav aria-label="Sections">
        <Link href="/about">About the harbor</Link>
        <Link href="/about#tides" className="secondary">
          Tide tables
        </Link>
      </nav>
    </main>
  );
}
