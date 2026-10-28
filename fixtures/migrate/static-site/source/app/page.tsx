import Link from 'next/link';
import { siteName } from '@/lib/site';
import { Badge } from '@/components/Badge';

export const dynamic = 'force-static';

// Rendered once at build time.
const year = 2026;

export default function HomePage() {
  return (
    <section className="hero">
      <h1>Welcome to {siteName}</h1>
      <p>Calm websites since {year}.</p>
      <Badge label="new" />
      <Link href="/about" className="more">About us</Link>
    </section>
  );
}
