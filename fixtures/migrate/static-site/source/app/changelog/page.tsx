import { Stamp } from '@/components/Stamp';
import { siteName } from '@/lib/site';

export const metadata = { title: 'Changelog' };

export default async function ChangelogPage() {
  // Resolved before the markup renders.
  const entries = await Promise.resolve(['First release']);
  const heading = `${siteName}: ${entries.length + offset} entries`;
  return (
    <section>
      <h1>{heading}</h1>
      <Stamp />
    </section>
  );
}

const offset = 1;
