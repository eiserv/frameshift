import { cookies } from 'next/headers';

export async function label() {
  return (await cookies()).get('label')?.value ?? 'none';
}
