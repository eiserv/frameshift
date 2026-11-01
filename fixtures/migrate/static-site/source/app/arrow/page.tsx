import { Card } from '@/components/Card';

export const metadata = { title: 'Arrow' };

const ArrowPage = () => (
  <main>
    <Card title="Declared later" />
    <Card title="Toned" tone="loud" />
  </main>
);

export default ArrowPage;
