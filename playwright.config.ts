import { defineConfig } from '@playwright/test'
export default defineConfig({testDir:'./tests', workers:1, use:{channel:'msedge',headless:true,viewport:{width:600,height:850}}, reporter:'list'})
