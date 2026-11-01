import Link from 'next/link';
import { siteName } from '@/lib/site';

export function SiteHeader() {
  return (
    <header>
      <Link href="/">{siteName}</Link>
    </header>
  );
}
