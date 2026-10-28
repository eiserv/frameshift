import Link from 'next/link';

export const metadata = { title: 'About' };

export default function AboutPage() {
  return (
    <section className="prose">
      <h1>About us</h1>
      <p>We build calm websites.</p>
      <Link href="/contact">Get in touch</Link>
    </section>
  );
}
