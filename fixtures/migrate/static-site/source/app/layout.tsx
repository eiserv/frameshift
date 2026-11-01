import './globals.css';
import type { Metadata } from 'next';
import { SiteHeader } from '@/components/SiteHeader';

export const metadata: Metadata = {
  title: { default: 'Lumen Studio', template: '%s | Lumen Studio' },
  description: 'A small, fictional design studio.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="page">
        <SiteHeader />
        <main>{children}</main>
      </body>
    </html>
  );
}
