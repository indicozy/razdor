// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// The wiki is published under the main site's gh-pages branch (see .github/workflows/wiki.yml):
// https://indicozy.github.io/razdor/wiki/
export default defineConfig({
	site: 'https://indicozy.github.io',
	base: '/razdor/wiki',
	integrations: [
		starlight({
			title: 'Razdor — вики',
			defaultLocale: 'root',
			locales: {
				root: { label: 'Русский', lang: 'ru' },
				en: { label: 'English', lang: 'en' },
			},
			favicon: '/favicon.ico',
			logo: {
				src: './public/icon-192.png',
				alt: 'Razdor',
			},
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/indicozy/razdor' },
				{ icon: 'discord', label: 'Discord', href: 'https://discord.gg/cdTtYz5dH' },
			],
			editLink: {
				baseUrl: 'https://github.com/indicozy/razdor/edit/main/wiki/',
			},
			customCss: ['./src/styles/razdor.css'],
			sidebar: [
				{ label: 'Начало', translations: { en: 'Getting started' }, items: [{ autogenerate: { directory: 'start' } }] },
				{ label: 'Уроки', translations: { en: 'Tutorials' }, items: [{ autogenerate: { directory: 'tutorials' } }] },
				{ label: 'Отличия от оригинала', translations: { en: 'Differences from the original' }, items: [{ autogenerate: { directory: 'differences' } }] },
				{ label: 'Моды', translations: { en: 'Mods' }, items: [{ autogenerate: { directory: 'mods' } }] },
				{ label: 'Инструменты', translations: { en: 'Tools' }, items: [{ autogenerate: { directory: 'tools' } }] },
				{ label: 'Телефоны', translations: { en: 'Phones' }, items: [{ autogenerate: { directory: 'phones' } }] },
				{ label: 'Вопросы', translations: { en: 'FAQ' }, items: [{ autogenerate: { directory: 'faq' } }] },
				{ label: 'Ссылки', translations: { en: 'Links' }, items: [{ autogenerate: { directory: 'links' } }] },
			],
		}),
	],
});
