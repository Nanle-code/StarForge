pipeline {
  agent any

  options {
    disableConcurrentBuilds()
    timestamps()
    timeout(time: 45, unit: 'MINUTES')
  }

  parameters {
    choice(name: 'ACTION', choices: ['verify', 'deploy', 'rollback'], description: 'Pipeline action')
    choice(name: 'TARGET_ENVIRONMENT', choices: ['staging', 'production'], description: 'Deployment target')
    string(name: 'WASM_PATH', defaultValue: '', description: 'Optional: path to compiled WASM for contract tests')
    string(name: 'CONTRACT_ID', defaultValue: '', description: 'Optional: contract ID for post-deploy monitoring')
  }

  stages {
    stage('Quality gate') {
      steps {
        sh '''#!/usr/bin/env bash
          set -euo pipefail
          rustup component add rustfmt clippy
          cargo fmt --all --check
          cargo build --locked
          cargo clippy --all-features --locked -- -D warnings
          cargo test --test cli_smoke --locked
          git diff --exit-code Cargo.lock
        '''
      }
    }

    stage('Contract tests') {
      steps {
        sh '''#!/usr/bin/env bash
          set -euo pipefail
          cargo test --locked -- --test-threads=1
        '''
      }
    }

    stage('Contract lint & audit') {
      when { expression { params.WASM_PATH != '' } }
      steps {
        sh '''#!/usr/bin/env bash
          set -euo pipefail
          cargo build --locked --release
          ./target/release/starforge lint --wasm "$WASM_PATH" || true
          ./target/release/starforge audit --wasm "$WASM_PATH" --output json > audit-results.json || true
        '''
        archiveArtifacts artifacts: 'audit-results.json', allowEmptyArchive: true
      }
    }

    stage('Deploy or rollback') {
      when { expression { params.ACTION != 'verify' } }
      steps {
        script {
          if (params.TARGET_ENVIRONMENT == 'production') {
            input message: "Approve production ${params.ACTION}?", ok: 'Approve'
          }
        }
        withCredentials([
          string(credentialsId: 'starforge-deploy-command',  variable: 'STARFORGE_DEPLOY_COMMAND'),
          string(credentialsId: 'starforge-rollback-command', variable: 'STARFORGE_ROLLBACK_COMMAND'),
          string(credentialsId: 'starforge-healthcheck-url',  variable: 'STARFORGE_HEALTHCHECK_URL'),
          string(credentialsId: 'starforge-notify-webhook',   variable: 'STARFORGE_NOTIFY_WEBHOOK')
        ]) {
          sh '''#!/usr/bin/env bash
            set -euo pipefail
            export STARFORGE_DEPLOY_ENVIRONMENT="$TARGET_ENVIRONMENT"
            export STARFORGE_NOTIFY_ACTOR="${BUILD_USER:-Jenkins}"
            export STARFORGE_NOTIFY_URL="${BUILD_URL}"
            if [ "$TARGET_ENVIRONMENT" = production ]; then
              export STARFORGE_DEPLOY_APPROVED=true STARFORGE_ROLLBACK_APPROVED=true
            fi
            if [ "$ACTION" = deploy ]; then
              bash scripts/ci-deploy.sh
            else
              bash scripts/ci-rollback.sh
            fi
          '''
        }
      }
    }

    stage('Post-deploy monitoring') {
      when {
        allOf {
          expression { params.ACTION == 'deploy' }
          expression { params.CONTRACT_ID != '' }
        }
      }
      steps {
        sh '''#!/usr/bin/env bash
          set -euo pipefail
          ./target/release/starforge inspect state "$CONTRACT_ID" \
            --network "${TARGET_ENVIRONMENT == 'production' ? 'mainnet' : 'testnet'}" \
            --json > monitor-snapshot.json || true
        '''
        archiveArtifacts artifacts: 'monitor-snapshot.json', allowEmptyArchive: true
      }
    }
  }

  post {
    success {
      withCredentials([string(credentialsId: 'starforge-notify-webhook', variable: 'STARFORGE_NOTIFY_WEBHOOK')]) {
        sh '''#!/usr/bin/env bash
          STARFORGE_NOTIFY_STATUS=success \
          STARFORGE_NOTIFY_ENV="$TARGET_ENVIRONMENT" \
          STARFORGE_NOTIFY_ACTOR="${BUILD_USER:-Jenkins}" \
          STARFORGE_NOTIFY_URL="$BUILD_URL" \
          bash scripts/ci-notify.sh || true
        '''
      }
    }
    failure {
      withCredentials([string(credentialsId: 'starforge-notify-webhook', variable: 'STARFORGE_NOTIFY_WEBHOOK')]) {
        sh '''#!/usr/bin/env bash
          STARFORGE_NOTIFY_STATUS=failure \
          STARFORGE_NOTIFY_ENV="$TARGET_ENVIRONMENT" \
          STARFORGE_NOTIFY_ACTOR="${BUILD_USER:-Jenkins}" \
          STARFORGE_NOTIFY_URL="$BUILD_URL" \
          bash scripts/ci-notify.sh || true
        '''
      }
    }
  }
}
