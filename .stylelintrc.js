import PostCssScss from 'postcss-scss'

export default {
  root: true,
  defaultSeverity: 'error',
  plugins: [
    'stylelint-scss',
    'stylelint-order',
    'stylelint-declaration-block-no-ignored-properties',
  ],
  extends: [
    'stylelint-config-standard',
    'stylelint-config-html/html', // the shareable html config for Stylelint.
    'stylelint-config-recess-order',
    // 'stylelint-config-prettier'
  ],
  rules: {
    'selector-pseudo-class-no-unknown': [
      true,
      { ignorePseudoClasses: ['global'] },
    ],
    'font-family-name-quotes': null,
    'font-family-no-missing-generic-family-keyword': null,
    'max-nesting-depth': [
      10,
      {
        ignore: ['blockless-at-rules', 'pseudo-classes'],
      },
    ],
    'declaration-block-no-duplicate-properties': true,
    'no-duplicate-selectors': true,
    'no-descending-specificity': null,
    'selector-class-pattern': null,
    'value-no-vendor-prefix': [true, { ignoreValues: ['box'] }],
    'at-rule-no-unknown': [
      true,
      {
        ignoreAtRules: [
          'tailwind',
          'unocss',
          'layer',
          'apply',
          'variants',
          'responsive',
          'screen',
        ],
      },
    ],
    'at-rule-no-deprecated': [
      true,
      {
        ignoreAtRules: ['apply'],
      },
    ],
  },
  overrides: [
    {
      files: ['**/*.scss', '*.scss'],
      customSyntax: PostCssScss,
      rules: {
        'at-rule-no-unknown': null,
        'import-notation': null,
        'scss/at-rule-no-unknown': [
          true,
          {
            ignoreAtRules: [
              'tailwind',
              'unocss',
              'layer',
              'apply',
              'variants',
              'responsive',
              'screen',
            ],
          },
        ],
      },
    },
  ],
}
