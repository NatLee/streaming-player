import sys
import logging
import os
from pathlib import Path
from datetime import timedelta

logger = logging.getLogger(__name__)

print("-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-")

# Build paths inside the project like this: BASE_DIR / 'subdir'.
BASE_DIR = Path(__file__).resolve().parent.parent

print(f"---------- Project DIR: {BASE_DIR}")

# SECURITY WARNING: keep the secret key used in production secret!
SECRET_KEY = (
    "django-insecure-natlee-9527-h0usnb4^27w+^)i*)f24$i3$$)(&1m@r0cj42wsi1n@9&+7@h)"
)

SITE_ID = 1

# ----------------------------- START - DEBUG setting -------------------------------
# SECURITY WARNING: don't run with debug turned on in production!
DEBUG = False
print(f"---------- Debug mode: {DEBUG}")
# ------------------------------ END - DEBUG setting --------------------------------


ALLOWED_HOSTS = ["*"]

# -------------- START - CORS Setting --------------
CORS_ALLOW_ALL_ORIGINS = True
CORS_ALLOW_CREDENTIALS = True
CSRF_TRUSTED_ORIGINS = [
    "http://127.0.0.1",
    "http://localhost",
    "https://natlee.info",
    "https://player.natlee.info",
]
# -------------- END - CORS Setting -----------------

# -------------- START - Google Auth Setting --------------
SECURE_REFERRER_POLICY = "no-referrer-when-downgrade"
# SECURE_CROSS_ORIGIN_OPENER_POLICY = "same-origin-allow-popups"
SECURE_CROSS_ORIGIN_OPENER_POLICY = None
SOCIAL_GOOGLE_CLIENT_ID = (
    "376808175534-d6mefo6b1kqih3grjjose2euree2g3cs.apps.googleusercontent.com"
)
LOGIN_REDIRECT_URL = "/"
VALID_REGISTER_DOMAINS = ["gmail.com"]
# --------------- END - Google Auth Setting -----------------

# Application definition

INSTALLED_APPS = [
    # admin skin
    "unfold",  # before django.contrib.admin
    "unfold.contrib.filters",  # optional, if special filters are needed
    "unfold.contrib.forms",  # optional, if special form elements are needed
    "unfold.contrib.inlines",  # optional, if special inlines are needed
    "unfold.contrib.import_export",  # optional, if django-import-export package is used
    #"unfold.contrib.guardian",  # optional, if django-guardian package is used
    "unfold.contrib.simple_history",  # optional, if django-simple-history package is used
    # websocket
    "daphne",
    "channels",
    "channels_redis",
    # django
    "django.contrib.admin",
    "django.contrib.auth",
    "django.contrib.contenttypes",
    "django.contrib.sessions",
    "django.contrib.messages",
    "django.contrib.staticfiles",
    # base
    "rest_framework",
    "drf_yasg",
    # package
    "bootstrap3",
    "corsheaders",
    "rangefilter",
    "simple_history",
    # custom
    "custom_jwt",
    "custom_auth",
    "userprofile",
    "player",
    # test
    "ping",
]

UNFOLD = {
    "SITE_TITLE": "Player Platform Admin",
    "SITE_HEADER": "Player Platform Admin",
    "THEME": "light", # Force theme: "dark" or "light". Will disable theme switcher
    "SIDEBAR": {
        "show_search": True,  # Search in applications and models names
        "show_all_applications": True,  # Dropdown with all applications and models
    }
}

ASGI_APPLICATION = "backend.asgi.application"


CHANNEL_LAYERS = {
    "default": {
        "BACKEND": "channels_redis.core.RedisChannelLayer",
        "CONFIG": {
            "hosts": [("backend-redis", 6379)],
        },
    },
}

MIDDLEWARE = [
    "django.middleware.security.SecurityMiddleware",
    "whitenoise.middleware.WhiteNoiseMiddleware",
    "django.contrib.sessions.middleware.SessionMiddleware",
    "corsheaders.middleware.CorsMiddleware",
    "django.middleware.common.CommonMiddleware",
    "django.middleware.csrf.CsrfViewMiddleware",
    "django.contrib.auth.middleware.AuthenticationMiddleware",
    "django.contrib.messages.middleware.MessageMiddleware",
    "django.middleware.clickjacking.XFrameOptionsMiddleware",
    "simple_history.middleware.HistoryRequestMiddleware",
]

ROOT_URLCONF = "backend.urls"

TEMPLATES = [
    {
        "BACKEND": "django.template.backends.django.DjangoTemplates",
        "DIRS": [
            os.path.join(BASE_DIR, "templates").replace("\\", "/")
        ],
        "APP_DIRS": True,
        "OPTIONS": {
            "context_processors": [
                "django.template.context_processors.debug",
                "django.template.context_processors.request",
                "django.contrib.auth.context_processors.auth",
                "django.contrib.messages.context_processors.messages",
            ],
        },
    },
]

# -------------- START - Swagger Setting --------------

USE_X_FORWARDED_HOST = True

SWAGGER_SETTINGS = {
    "SECURITY_DEFINITIONS": {
        "Token(add prefix `Bearer` yourself)": {
            "type": "apiKey",
            "name": "Authorization",
            "in": "header",
        }
    },
    "LOGIN_URL": "/api/__hidden_admin/login",
    "LOGOUT_URL": "/api/__hidden_admin/logout/?next=/api/__hidden_swagger",
}

# --------------- END - Swagger Setting----------------


# -------------- START - Database Setting --------------
SQLITE_DIR = BASE_DIR / "db.sqlite3"
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.sqlite3",
        "NAME": SQLITE_DIR,
    }
}
print(f"---------- SQLITE DIR: {SQLITE_DIR}")
# -------------- END - Database Setting --------------


# Password validation
# https://docs.djangoproject.com/en/3.2/ref/settings/#auth-password-validators

AUTH_PASSWORD_VALIDATORS = [
    {
        "NAME": "django.contrib.auth.password_validation.UserAttributeSimilarityValidator",
    },
    {
        "NAME": "django.contrib.auth.password_validation.MinimumLengthValidator",
    },
    {
        "NAME": "django.contrib.auth.password_validation.CommonPasswordValidator",
    },
    {
        "NAME": "django.contrib.auth.password_validation.NumericPasswordValidator",
    },
]


# Internationalization
# https://docs.djangoproject.com/en/3.2/topics/i18n/

from django.conf.locale.en import formats as en_formats
en_formats.DATETIME_FORMAT = "Y/m/d H:i:s"

LANGUAGE_CODE = "en-us"
TIME_ZONE = "Asia/Taipei"
USE_I18N = True
USE_L10N = True
USE_TZ = True
APPEND_SLASH = False

# Static files (CSS, JavaScript, Images)
STATIC_URL = "api/__hidden_statics/"
STATIC_ROOT = "staticfiles"


# Default primary key field type
DEFAULT_AUTO_FIELD = "django.db.models.BigAutoField"

# -------------- START - Log setting --------------
LOG_ROOT = Path(BASE_DIR) / 'logs'
LOG_ROOT.mkdir(exist_ok=True)

LOG_TYPES = {
    'file': 'standard',
    'database': 'standard',
    'api': 'api-format',
    'auth': 'standard',
}

FORMATTERS = {
    'standard': {
        'format': '[%(asctime)s] [%(filename)s:%(lineno)d] [%(module)s:%(funcName)s] [%(levelname)s] - %(message)s'
    },
    "simple": {
        "format": "%(levelname)s %(message)s"
    },
    "api-format": {
        "format": "[%(asctime)s] [%(funcName)s:%(lineno)d] [%(levelname)s] - %(message)s"
    },
}

for log_type in LOG_TYPES.keys():
    log_type_path = Path(BASE_DIR) / 'logs' / log_type
    log_type_path.mkdir(exist_ok=True)

HANDLERS = {}

for log_type, formatter in LOG_TYPES.items():
    HANDLERS[log_type] = {
        'class': 'common.log.InterceptTimedRotatingFileHandler',
        'filename': f"{LOG_ROOT / log_type / f'{log_type}.log'}",
        'when': "H",
        'interval': 1,
        'backupCount': 1,
        'formatter': formatter,
        'encoding': 'utf-8',
    }

LOGGING = {
    'version': 1,
    'disable_existing_loggers': True,
    'formatters': FORMATTERS,
    'filters': {
    },
    'handlers': HANDLERS,
    'loggers': {
        'django': {
            'handlers': ['file'],
            'propagate': True,
            'level': "INFO"
        },
        '''
        'django.db.backends': {
            'handlers': ['database'],
            'propagate': False,
            'level': "INFO"
        },
        '''
        'django.request': {
            'handlers': ['file'],
            'propagate': False,
            'level': "DEBUG"
        },
        "custom_auth": {
            "handlers": ["auth"],
            "propagate": False,
            "level": "DEBUG"
        },
        # JWT相關
        "custom_jwt": {
            "handlers": ["auth"],
            "propagate": False,
            "level": "DEBUG"
        },
    }
}


# --------------- END - Log setting ---------------


# ---------------------------- START - REST_FRAMEWORK setting --------------------------
REST_FRAMEWORK = {
    "DEFAULT_AUTHENTICATION_CLASSES": [
        # 支援使用simplejwt的JWT登入
        "rest_framework_simplejwt.authentication.JWTAuthentication",
        # 支援一般session進行後台登入
        "rest_framework.authentication.SessionAuthentication",
    ],
    "DEFAULT_PERMISSION_CLASSES": (
        # restframework的API必須登入才可使用
        "rest_framework.permissions.IsAuthenticated",
    ),
    # 使用JSON來render API而非HTML界面
    "DEFAULT_RENDERER_CLASSES": ("rest_framework.renderers.JSONRenderer",),
}
# ----------------------------- END - REST_FRAMEWORK setting ----------------------------


# -------------- Start - SimpleJWT Setting --------------
SIMPLE_JWT = {
    "ACCESS_TOKEN_LIFETIME": timedelta(minutes=3600),
    "REFRESH_TOKEN_LIFETIME": timedelta(days=1),
    "ROTATE_REFRESH_TOKENS": False,
    "BLACKLIST_AFTER_ROTATION": False,
    "UPDATE_LAST_LOGIN": False,
    "ALGORITHM": "HS256",
    "SIGNING_KEY": SECRET_KEY,
    "VERIFYING_KEY": None,
    "AUDIENCE": None,
    "ISSUER": None,
    "JWK_URL": None,
    "LEEWAY": 0,
    "AUTH_HEADER_TYPES": ("Bearer",),
    "AUTH_HEADER_NAME": "HTTP_AUTHORIZATION",
    "USER_ID_FIELD": "id",
    "USER_ID_CLAIM": "user_id",
    "USER_AUTHENTICATION_RULE": "rest_framework_simplejwt.authentication.default_user_authentication_rule",
    "AUTH_TOKEN_CLASSES": ("rest_framework_simplejwt.tokens.AccessToken",),
    "TOKEN_TYPE_CLAIM": "token_type",
    "TOKEN_USER_CLASS": "rest_framework_simplejwt.models.TokenUser",
    "JTI_CLAIM": "jti",
    "SLIDING_TOKEN_REFRESH_EXP_CLAIM": "refresh_exp",
    "SLIDING_TOKEN_LIFETIME": timedelta(minutes=5),
    "SLIDING_TOKEN_REFRESH_LIFETIME": timedelta(days=1),
}
# -------------- END - SimpleJWT Setting --------------


print("-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-*-")
