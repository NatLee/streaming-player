class BaseException(Exception):
    def __init__(self, message):
        super(BaseException, self).__init__(message)


class RequestInputParserError(BaseException):
    pass
