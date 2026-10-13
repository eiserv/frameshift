import Link from 'next/link';

// Opening hours are fixed for this fictional harbor office.
const OPENING_HOURS = '8:00 to 16:00';

/** The about page of the harbor office. */
export default function AboutPage() {
  // Shown next to the opening hours.
  const office = 'Pier 3';
  return (
    <article className="about">
      <h1>About the harbor</h1>
      <p>
        The office at {office} is open from {OPENING_HOURS}.
      </p>
      <h2 id="tides">Tides &amp; currents</h2>
      <p>Tables are posted daily.</p>
      <Link href="/">Back to the start page</Link>
    </article>
  );
}
